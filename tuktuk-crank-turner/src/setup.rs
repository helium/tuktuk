use anyhow::Result;
use std::fs;
use std::path::PathBuf;

const DEFAULT_CONFIG: &str = r#"# RPC endpoint URL
rpc_url = "https://api.mainnet-beta.solana.com"

# Path to your Solana keypair file
key_path = "~/.config/solana/id.json"

# Minimum crank fee in lamports
min_crank_fee = 10000
"#;

pub fn get_default_config_path() -> Option<PathBuf> {
    let home_dir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;

    Some(
        PathBuf::from(home_dir)
            .join(".config")
            .join("helium")
            .join("cli")
            .join("tuktuk-crank-turner")
            .join("config.toml"),
    )
}

fn write_default_config(config_path: &PathBuf) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(config_path, DEFAULT_CONFIG)?;

    Ok(())
}

pub fn create_config_if_missing() -> Result<()> {
    let Some(config_path) = get_default_config_path() else {
        eprintln!("Warning: Could not determine home directory. Skipping config creation.");
        return Ok(());
    };

    if !config_path.exists() {
        write_default_config(&config_path)?;
    }

    Ok(())
}
