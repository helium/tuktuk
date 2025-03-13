use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use tuktuk_program::cron::{
    accounts::{CronJobTransactionV0, CronJobV0},
    types::{AddCronTransactionArgsV0, RemoveCronTransactionArgsV0, TransactionSourceV0},
};
use tuktuk_sdk::prelude::*;

use super::{cron::CronArg, TransactionSource};
use crate::{
    client::send_instructions,
    cmd::Opts,
    result::Result,
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct CronTransactionCmd {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    CreateRemote {
        #[command(flatten)]
        cron_job: CronArg,
        #[arg(long)]
        index: u32,
        #[arg(long)]
        url: String,
        #[arg(long)]
        signer: Pubkey,
    },
    Close {
        #[command(flatten)]
        cron_job: CronArg,
        #[arg(long)]
        id: u32,
    },
    Get {
        #[command(flatten)]
        cron_job: CronArg,
        #[arg(long)]
        id: u32,
    },
    List {
        #[command(flatten)]
        cron_job: CronArg,
    },
}

impl CronTransactionCmd {
    pub async fn run(&self, opts: Opts) -> Result<()> {
        match &self.cmd {
            Cmd::CreateRemote {
                cron_job,
                index,
                url,
                signer,
            } => {
                let client = opts.client().await?;
                let cron_job_key = cron_job.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let (cron_job_transaction_key, ix) = tuktuk::cron_job_transaction::add_transaction(
                    client.payer.pubkey(),
                    cron_job_key,
                    AddCronTransactionArgsV0 {
                        index: *index,
                        transaction_source: TransactionSourceV0::RemoteV0 {
                            url: url.to_string(),
                            signer: *signer,
                        },
                    },
                )?;
                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    &[ix],
                    &[],
                )
                .await?;
                print_json(&CronTransaction {
                    pubkey: cron_job_transaction_key,
                    id: *index,
                    cron_job: cron_job_key,
                    transaction_source: Some(TransactionSource::RemoteV0 {
                        url: url.to_string(),
                        signer: *signer,
                    }),
                })?;
            }
            Cmd::Close {
                cron_job,
                id: index,
            } => {
                let client = opts.client().await?;
                let cron_job_key = cron_job.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let cron_job_transaction_key =
                    tuktuk::cron_job_transaction::key(&cron_job_key, *index);
                let cron_job_transaction: CronJobTransactionV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_transaction_key)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Cron job transaction not found: {}",
                            cron_job_transaction_key
                        )
                    })?;
                let ix = tuktuk::cron_job_transaction::remove_transaction(
                    client.payer.pubkey(),
                    cron_job_key,
                    RemoveCronTransactionArgsV0 { index: *index },
                )?;
                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    &[ix],
                    &[],
                )
                .await?;
                print_json(&CronTransaction {
                    pubkey: cron_job_transaction_key,
                    id: *index,
                    cron_job: cron_job_key,
                    transaction_source: Some(TransactionSource::from(to_transaction_source(
                        cron_job_transaction.transaction,
                    ))),
                })?;
            }
            Cmd::Get {
                cron_job,
                id: index,
            } => {
                let client = opts.client().await?;
                let cron_job_key = cron_job.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let cron_job_transaction_key =
                    tuktuk::cron_job_transaction::key(&cron_job_key, *index);
                let cron_job_transaction: CronJobTransactionV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_transaction_key)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Cron job transaction not found: {}",
                            cron_job_transaction_key
                        )
                    })?;
                print_json(&CronTransaction {
                    pubkey: cron_job_transaction_key,
                    id: *index,
                    cron_job: cron_job_key,
                    transaction_source: Some(TransactionSource::from(to_transaction_source(
                        cron_job_transaction.transaction,
                    ))),
                })?;
            }
            Cmd::List { cron_job } => {
                let client = opts.client().await?;
                let cron_job_key = cron_job.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let cron_job: CronJobV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Cron job not found: {}", cron_job_key))?;
                let cron_job_transaction_keys =
                    tuktuk::cron_job_transaction::keys(&cron_job_key, &cron_job)?;
                let cron_job_transactions = client
                    .as_ref()
                    .anchor_accounts::<CronJobTransactionV0>(&cron_job_transaction_keys)
                    .await?;
                print_json(
                    &cron_job_transactions
                        .iter()
                        .map(|(pubkey, maybe_t)| {
                            maybe_t.as_ref().map(|t| CronTransaction {
                                pubkey: *pubkey,
                                id: t.id,
                                cron_job: cron_job_key,
                                transaction_source: Some(TransactionSource::from(
                                    to_transaction_source(t.transaction.clone()),
                                )),
                            })
                        })
                        .collect::<Vec<_>>(),
                )?;
            }
        }
        Ok(())
    }
}

fn to_transaction_source(source: TransactionSourceV0) -> tuktuk_program::TransactionSourceV0 {
    match source {
        TransactionSourceV0::RemoteV0 { url, signer } => {
            tuktuk_program::TransactionSourceV0::RemoteV0 { url, signer }
        }
        TransactionSourceV0::CompiledV0(transaction) => {
            tuktuk_program::TransactionSourceV0::CompiledV0(tuktuk_program::CompiledTransactionV0 {
                num_rw_signers: transaction.num_rw_signers,
                num_ro_signers: transaction.num_ro_signers,
                num_rw: transaction.num_rw,
                accounts: transaction.accounts,
                instructions: transaction
                    .instructions
                    .into_iter()
                    .map(|i| tuktuk_program::CompiledInstructionV0 {
                        program_id_index: i.program_id_index,
                        accounts: i.accounts,
                        data: i.data,
                    })
                    .collect(),
                signer_seeds: transaction.signer_seeds,
            })
        }
    }
}

#[derive(Serialize)]
pub struct CronTransaction {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    pub id: u32,
    #[serde(with = "serde_pubkey")]
    pub cron_job: Pubkey,
    pub transaction_source: Option<TransactionSource>,
}
