import { execSync } from "child_process";

export const ANCHOR_PATH = "anchor";

export async function ensureIdls() {
  let programs = [
    {
      name: "tuktuk",
      pid: "tuktukUrfhXT6ZT77QTU8RQtvgL967uRuVagWF57zVA",
    },
    {
      name: "cpi_example",
      pid: "cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC",
    },
    {
      name: "cron",
      pid: "cronAjRZnJn3MTP3B9kE62NWDrjSuAPVXf9c4hu4grM",
    },
  ];
  await Promise.all(
    programs.map(async (program) => {
      try {
        execSync(
          `${ANCHOR_PATH} idl init --filepath ${__dirname}/../target/idl/${program.name}.json ${program.pid}`,
          { stdio: "inherit", shell: "/bin/bash" }
        );
      } catch {
        execSync(
          `${ANCHOR_PATH} idl upgrade --filepath ${__dirname}/../target/idl/${program.name}.json ${program.pid}`,
          { stdio: "inherit", shell: "/bin/bash" }
        );
      }
    })
  );
}

export function makeid(length: number) {
  let result = "";
  const characters =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  const charactersLength = characters.length;
  let counter = 0;
  while (counter < length) {
    result += characters.charAt(Math.floor(Math.random() * charactersLength));
    counter += 1;
  }
  return result;
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
