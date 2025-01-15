use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use tuktuk_program::types::InitializeTuktukConfigArgsV0;
use tuktuk_sdk::prelude::*;

use crate::{
    client::send_instructions,
    cmd::Opts,
    result::Result,
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct TuktukConfigCmd {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    Create {
        #[arg(long)]
        authority: Option<Pubkey>,
        #[arg(long, help = "Minimum deposit in bones to create a task queue")]
        min_deposit: u64,
    },
}

impl TuktukConfigCmd {
    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::Create {
                authority,
                min_deposit,
            } => {
                let client = opts.client().await?;
                let tuktuk_config_key = tuktuk::config_key();

                let mut combined_ixs = Vec::new();

                let extra_signers = Vec::new();

                // Combine existing instructions with mint instructions if created
                combined_ixs.push(tuktuk::create_config(
                    client.payer.pubkey(),
                    *authority,
                    InitializeTuktukConfigArgsV0 {
                        min_deposit: *min_deposit,
                    },
                )?);

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    combined_ixs,
                    &extra_signers,
                )
                .await?;

                let tuktuk_config: tuktuk_program::TuktukConfigV0 = client
                    .as_ref()
                    .anchor_account(&tuktuk_config_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Tuktuk config account not found"))?;

                print_json(&TuktukConfig {
                    pubkey: tuktuk_config_key,
                    authority: tuktuk_config.authority,
                    bump_seed: tuktuk_config.bump_seed,
                })?;
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct TuktukConfig {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub authority: Pubkey,
    pub bump_seed: u8,
}
