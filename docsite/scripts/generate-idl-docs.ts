const fs = require("fs");
const path = require("path");
const prettier = require("prettier");

// Path configuration
const docSitePath = path.join(__dirname, "..", "src", "pages", "docs", "api");
const docSiteNavigationStart = path.join(__dirname, "..", "src", "data");

/**
 * Type definitions for the IDL structure
 */
interface Type {
  name?: string;
  type?: Type;
  kind?: string;
  fields?: {
    name: string;
    type: Type;
    docs?: string[];
  }[];
  variants?: {
    name: string;
    fields?: {
      name: string;
      type: Type;
    }[];
  }[];
  defined?: {
    name: string;
  };
  vec?: Type;
  option?: Type;
  array?: [string, number];
}

/**
 * Generates markdown documentation for a single IDL file
 * @param idlJson - The parsed IDL JSON object
 * @returns Markdown string containing the documentation
 */
const generateIdlDocs = (idlJson: any) => {
  const { instructions, accounts, types, errors } = idlJson;
  const realFileName = formatFileName(idlJson.metadata.name);

  let mdFile = generateHeader(realFileName);
  mdFile += generateInstructionsSection(instructions, types);
  mdFile += generateAccountsSection(accounts, types);
  mdFile += generateTypesSection(types);
  mdFile += generateErrorsSection(errors);
  
  return mdFile;
};

/**
 * Formats a file name for display
 * @param name - The raw file name
 * @returns Formatted file name
 */
const formatFileName = (name: string): string => {
  return name
    .split(".")[0]
    .replace(/_/g, " ")
    .replace(/(^|\s)\S/g, (L) => L.toUpperCase());
};

/**
 * Generates the header section of the documentation
 * @param title - The title of the documentation
 * @returns Markdown string for the header
 */
const generateHeader = (title: string): string => {
  return `# ${title} SDK

  {% callout title="Quick tip" %}
If you are looking for a quick start guide, check out the [Quickstart](/docs/learn/quickstart) guide.
{% /callout %}

## Instructions

`;
};

/**
 * Generates the instructions section of the documentation
 * @param instructions - Array of instruction definitions
 * @param types - Array of type definitions
 * @returns Markdown string for the instructions section
 */
const generateInstructionsSection = (instructions: any[], types: Type[]): string => {
  let section = '';
  
  instructions.forEach((instruction) => {
    section += `### ${instruction.name}\n\n`;
    section += generateAccountsTable(instruction.accounts);
    section += generateArgsTable(instruction.args, types);
    section += '\n';
  });
  
  return section;
};

/**
 * Generates a markdown table for instruction accounts
 * @param accounts - Array of account definitions
 * @returns Markdown string for the accounts table
 */
const generateAccountsTable = (accounts: any[]): string => {
  let table = `#### Accounts\n\n`;
  table += `| Name | Mutability | Signer | Docs |\n`;
  table += `| ---- | ---------- | ------ | ---- |\n`;
  
  accounts.forEach((account) => {
    table += `| ${account.name} | ${account.writable ? "mut" : "immut"} | ${
      account.signer ? "yes" : "no"
    } | ${account.docs ? account.docs.join(" ") : ""} |\n`;
  });
  
  return table + '\n';
};

/**
 * Generates a markdown table for instruction arguments
 * @param args - Array of argument definitions
 * @param types - Array of type definitions
 * @returns Markdown string for the args table
 */
const generateArgsTable = (args: any[], types: Type[]): string => {
  if (!args || args.length === 0) return '';
  
  let table = `#### Args\n\n`;
  
  // For each argument, we'll create a separate section
  args.forEach((arg) => {
    table += `| Name | Type | Docs |\n`;
    table += `| ---- | ---- | ---- |\n`;
    
    // Basic argument information
    const typeName = arg.type.defined ? arg.type.defined.name : 'Unknown';
    table += `| ${arg.name} | ${typeName} | ${arg.docs ? arg.docs.join(" ") : ""} |\n\n`;
    
    // If it's a defined type, look up its details
    if (arg.type.defined) {
      const definedType = types.find(t => t.name === arg.type.defined.name);
      if (definedType && definedType.type) {
        // Handle different defined type kinds
        if (definedType.type.kind === 'struct') {
          table += `**${typeName} Fields:**\n\n`;
          table += `| Field | Type | Description |\n`;
          table += `| ----- | ---- | ----------- |\n`;
          
          definedType.type.fields?.forEach(field => {
            let fieldType = getFieldTypeName(field.type, types);
            table += `| ${field.name} | ${fieldType} | ${field.docs ? field.docs.join(" ") : ""} |\n`;
          });
          table += '\n';
        } else if (definedType.type.kind === 'enum') {
          table += `**${typeName} Variants:**\n\n`;
          table += `| Variant | Fields | Description |\n`;
          table += `| ------- | ------ | ----------- |\n`;
          
          definedType.type.variants?.forEach(variant => {
            let fields = '';
            if (variant.fields && variant.fields.length > 0) {
              fields = variant.fields.map(field => {
                if (typeof field === 'string') return field;
                let fieldType = getFieldTypeName(field.type, types);
                return field.name ? `${field.name}: ${fieldType}` : fieldType;
              }).join(", ");
            }
            table += `| ${variant.name} | ${fields} | |\n`;
          });
          table += '\n';
        }
      }
    }
  });
  
  return table;
};

/**
 * Helper function to get a readable type name for a field
 * @param type - The field type
 * @param types - Array of all available types
 * @returns A string representation of the type
 */
const getFieldTypeName = (type: any, types: Type[]): string => {
  if (!type) return 'unknown';
  
  // Handle direct primitive types
  if (typeof type === 'string') return type;
  
  // Handle defined types
  if (type.defined) {
    return type.defined.name;
  }
  
  // Handle complex types
  if (type.vec) {
    if (typeof type.vec === 'string') {
      return `Vec<${type.vec}>`;
    } else if (type.vec.defined) {
      return `Vec<${type.vec.defined.name}>`;
    } else {
      return 'Vec<unknown>';
    }
  }
  
  if (type.option) {
    if (typeof type.option === 'string') {
      return `Option<${type.option}>`;
    } else if (type.option.defined) {
      return `Option<${type.option.defined.name}>`;
    } else {
      return 'Option<unknown>';
    }
  }
  
  if (type.array) {
    return `[${type.array[0]}; ${type.array[1]}]`;
  }
  
  // Handle enum and struct types
  if (type.kind === 'enum') {
    return type.name || 'enum';
  }
  
  if (type.kind === 'struct') {
    return type.name || 'struct';
  }
  
  return 'unknown';
};

/**
 * Generates the accounts section of the documentation
 * @param accounts - Array of account definitions
 * @param types - Array of type definitions
 * @returns Markdown string for the accounts section
 */
const generateAccountsSection = (accounts: any[], types: Type[]): string => {
  if (!accounts || accounts.length === 0) return '';
  
  let section = `## Accounts\n\n`;
  
  accounts?.forEach((account) => {
    section += `### ${account.name}\n\n`;
    const accountType = types.find((t) => t.name === account.name);
    if (accountType?.type) {
      if (accountType.type.kind === 'struct') {
        section += generateStructTable(accountType.type, types);
      } else if (accountType.type.kind === 'enum') {
        section += generateEnumTable(accountType.type, types);
      } else {
        section += `Type: ${generateType(accountType.type, types)}\n`;
      }
    } else {
      section += "struct\n";
    }
    section += '\n';
  });
  
  return section;
};

/**
 * Generates the types section of the documentation
 * @param types - Array of type definitions
 * @returns Markdown string for the types section
 */
const generateTypesSection = (types: Type[]): string => {
  if (!types || types.length === 0) return '';
  
  let section = `## Types\n\n`;
  
  types.forEach((type) => {
    section += `### ${type.name}\n\n`;
    if (type.type) {
      if (type.type.kind === 'struct') {
        section += generateStructTable(type.type, types);
      } else if (type.type.kind === 'enum') {
        section += generateEnumTable(type.type, types);
      } else {
        section += `Type: ${generateType(type.type, types)}\n`;
      }
    } else {
      section += type.name ? `${type.name}\n` : "No type definition found.\n";
    }
    section += '\n';
  });
  
  return section;
};

/**
 * Generates a markdown table for a struct type
 * @param type - The struct type definition
 * @param types - Array of all available types
 * @returns Markdown string for the struct table
 */
const generateStructTable = (type: Type, types: Type[]): string => {
  if (!type.fields || type.fields.length === 0) {
    return "struct (no fields)\n";
  }
  
  let table = `| Field | Type | Description |\n`;
  table += `| ----- | ---- | ----------- |\n`;
  
  type.fields.forEach((field) => {
    if (!field.type) {
      table += `| ${field.name} | unknown | ${field.docs ? field.docs.join(" ") : ""} |\n`;
      return;
    }
    
    const fieldType = getFieldTypeName(field.type, types);
    table += `| ${field.name} | ${fieldType} | ${field.docs ? field.docs.join(" ") : ""} |\n`;
  });
  
  return table;
};

/**
 * Generates a markdown table for an enum type
 * @param type - The enum type definition
 * @param types - Array of all available types
 * @returns Markdown string for the enum table
 */
const generateEnumTable = (type: Type, types: Type[]): string => {
  if (!type.variants || type.variants.length === 0) {
    return "enum (no variants)\n";
  }
  
  let table = `| Variant | Fields | Description |\n`;
  table += `| ------- | ------ | ----------- |\n`;
  
  type.variants.forEach((variant) => {
    let fields = '';
    if (variant.fields && variant.fields.length > 0) {
      fields = variant.fields.map(field => {
        if (typeof field === 'string') return field;
        let fieldType = getFieldTypeName(field.type, types);
        return field.name ? `${field.name}: ${fieldType}` : fieldType;
      }).join(", ");
    }
    table += `| ${variant.name} | ${fields} | |\n`;
  });
  
  return table;
};

/**
 * Generates markdown representation of a type
 * @param type - The type to generate documentation for
 * @param types - Array of all available types for reference
 * @returns Markdown string representing the type
 */
const generateType = (type: Type | string, types: Type[]): string => {
  // Handle basic type cases
  if (typeof type === 'string') return type;
  if (!type) return 'unknown';

  // Handle defined types
  if (type.defined) {
    return type.defined.name || 'defined';
  }

  // Handle different type kinds
  switch (type.kind) {
    case "enum":
      return type.name || 'enum';
    case "struct":
      return type.name || 'struct';
    default:
      // Handle complex types like arrays, vectors, options
      if (type.vec) {
        const vecType = typeof type.vec === 'string' 
          ? type.vec 
          : (type.vec.defined?.name || 'type');
        return `Vec<${vecType}>`;
      }
      
      if (type.option) {
        const optionType = typeof type.option === 'string'
          ? type.option
          : (type.option.defined?.name || 'type');
        return `Option<${optionType}>`;
      }
      
      if (type.array) {
        return `[${type.array[0]}; ${type.array[1]}]`;
      }
      
      return 'unknown';
  }
};

/**
 * Adds a file to the navigation menu
 * @param fileName - The name of the file to add
 */
const addFileToNavigation = async (fileName: string) => {
  const navigationStart = fs.readFileSync(
    `${docSiteNavigationStart}/navigation.js`,
    "utf8"
  );

  const navigationStartSplit = navigationStart.split("// DOCS NAVIGATION START");
  const title = formatFileName(fileName.replace("-sdk", ""));

  const newNavigation = `${navigationStartSplit[0]}// DOCS NAVIGATION START
  { title: '${title}', href: '/docs/api/${fileName}' },\n${navigationStartSplit[1]}`;

  const formattedContent = await prettier.format(newNavigation, {
    semi: false,
    parser: "babel",
  });

  fs.writeFileSync(`${docSiteNavigationStart}/navigation.js`, formattedContent);
};

/**
 * Clears the navigation menu
 */
const clearNavigation = async () => {
  const navigationStart = fs.readFileSync(
    `${docSiteNavigationStart}/navigation.js`,
    "utf8"
  );
  
  const navigationStartSplitEnd = navigationStart.split("// DOCS NAVIGATION END");
  const navStart = navigationStartSplitEnd[0].split("// DOCS NAVIGATION START");

  const newNavigation = `${navStart[0]}
        // DOCS NAVIGATION START
        // DOCS NAVIGATION END${navigationStartSplitEnd[1]}`;

  const formattedContent = await prettier.format(newNavigation, {
    semi: false,
    parser: "babel",
  });

  fs.writeFileSync(`${docSiteNavigationStart}/navigation.js`, formattedContent);
};

/**
 * Main function to generate documentation for all IDL files
 */
const generateAllIdlDocs = async () => {
  const idlFiles: string[] = fs.readdirSync("../solana-programs/target/idl");
  await clearNavigation();

  for (const fileName of idlFiles) {
    const realFileName = fileName.split(".")[0].replace(/_/g, "-") + "-sdk";
    console.log(`Generating docs for ${realFileName}`);
    
    const idlJson = JSON.parse(
      fs.readFileSync(`../solana-programs/target/idl/${fileName}`, "utf8")
    );
    
    const mdFile = generateIdlDocs(idlJson);
    const formattedContent = await prettier.format(mdFile, {
      semi: false,
      parser: "markdown",
    });

    fs.writeFileSync(
      `${docSitePath}/${realFileName}.md`,
      formattedContent
    );
    
    await addFileToNavigation(realFileName);
  }
};

/**
 * Generates the errors section of the documentation
 * @param errors - Array of error definitions
 * @returns Markdown string for the errors section
 */
const generateErrorsSection = (errors: any[]): string => {
  if (!errors || errors.length === 0) return '';
  
  let section = `## Errors\n\n`;
  section += `| Code | Name | Message |\n`;
  section += `| ---- | ---- | ------- |\n`;
  
  errors.forEach((error) => {
    section += `| ${error.code} | ${error.name} | ${error.msg || ''} |\n`;
  });
  
  return section + '\n';
};

// Execute the main function
generateAllIdlDocs().catch(console.error);
