use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{program_pack::Pack, pubkey::Pubkey, signature::Keypair, signer::Signer};
use spl_token::state::Mint;
use tuktuk::types::InitializeTuktukConfigArgsV0;
use tuktuk_sdk::prelude::*;

use crate::{
    client::send_instructions,
    cmd::Opts,
    result::Result,
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct TuktukConfigCmd {
    #[arg(long, default_value = "false")]
    pub detailed: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    Create {
        #[arg(long)]
        authority: Option<Pubkey>,
        #[arg(long)]
        network_mint: Option<Pubkey>,
        #[arg(long, help = "Minimum deposit in bones to create a task queue")]
        min_deposit: u64,
    },
}

impl TuktukConfigCmd {
    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::Create {
                network_mint,
                authority,
                min_deposit,
            } => {
                let client = opts.client().await?;
                let tuktuk_config_key = tuktuk::config_key();

                let mut combined_ixs = Vec::new();

                let mut extra_signers = Vec::new();

                let network_mint_actual = match *network_mint {
                    Some(mint) => mint,
                    None => {
                        let mint_key = Keypair::new();
                        let mint_pubkey = mint_key.pubkey();
                        extra_signers.push(mint_key);
                        let mint_rent = client
                            .as_ref()
                            .get_minimum_balance_for_rent_exemption(Mint::LEN)
                            .await?;
                        let create_acc_ix = solana_program::system_instruction::create_account(
                            &client.payer.pubkey(),
                            &mint_pubkey,
                            mint_rent,
                            Mint::LEN as u64,
                            &spl_token::ID,
                        );
                        let create_mint_ixs = spl_token::instruction::initialize_mint(
                            &spl_token::ID,
                            &mint_pubkey,
                            &client.payer.pubkey(),
                            None,
                            8,
                        )?;

                        let create_ata_ix =
                    spl_associated_token_account::instruction::create_associated_token_account(
                        &client.payer.pubkey(),
                        &client.payer.pubkey(),
                        &mint_pubkey,
                        &spl_token::ID,
                    );
                        let mint_to_ix = spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &mint_pubkey,
                            &spl_associated_token_account::get_associated_token_address(
                                &client.payer.pubkey(),
                                &mint_pubkey,
                            ),
                            &client.payer.pubkey(),
                            &[],
                            10_000_000_000_000, // Mint 100,000 tokens
                        )?;

                        combined_ixs.push(create_acc_ix);
                        combined_ixs.push(create_mint_ixs);
                        combined_ixs.push(create_ata_ix);
                        combined_ixs.push(mint_to_ix);
                        mint_pubkey
                    }
                };

                // Combine existing instructions with mint instructions if created
                combined_ixs.push(tuktuk::create_config(
                    client.payer.pubkey(),
                    network_mint_actual,
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

                let tuktuk_config: tuktuk::TuktukConfigV0 = client
                    .as_ref()
                    .anchor_account(&tuktuk_config_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Tuktuk config account not found"))?;

                print_json(&TuktukConfig {
                    pubkey: tuktuk_config_key,
                    network_mint: network_mint_actual,
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
    pub network_mint: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub authority: Pubkey,
    pub bump_seed: u8,
}
