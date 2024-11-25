use std::str::FromStr;

use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use tuktuk::{tuktuk::accounts::TaskQueueV0, TuktukConfigV0};
use tuktuk_sdk::prelude::*;

use crate::{
    client::{send_instructions, CliClient},
    cmd::Opts,
    result::Result,
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct TaskQueueCmd {
    #[arg(long, default_value = "false")]
    pub detailed: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    Create {
        #[arg(long)]
        queue_authority: Option<Pubkey>,
        #[arg(long)]
        update_authority: Option<Pubkey>,
        #[arg(long)]
        capacity: u16,
        #[arg(long)]
        name: String,
    },
    Get {
        #[command(flatten)]
        task_queue: TaskQueueArg,
    },
    Fund {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(long, help = "Amount to fund the task queue with, in bones")]
        amount: u64,
    },
}

#[derive(Debug, Args)]
pub struct TaskQueueArg {
    #[arg(long = "task-queue-name", name = "task-queue-name")]
    pub name: Option<String>,
    #[arg(long = "task-queue-id", name = "task-queue-id")]
    pub id: Option<u32>,
    #[arg(long = "task-queue-pubkey", name = "task-queue-pubkey")]
    pub pubkey: Option<String>,
}

impl TaskQueueArg {
    pub async fn get_pubkey(&self, client: &CliClient) -> Result<Option<Pubkey>> {
        let tuktuk_config_key = tuktuk::config_key();

        if let Some(pubkey) = &self.pubkey {
            // Use the provided pubkey directly
            Ok(Some(Pubkey::from_str(pubkey)?))
        } else if let Some(id) = self.id {
            Ok(Some(tuktuk::task_queue::key(&tuktuk_config_key, id)))
        } else if let Some(name) = &self.name {
            let mapping: tuktuk::TaskQueueNameMappingV0 = client
                .as_ref()
                .anchor_account(&tuktuk::task_queue_name_mapping_key(
                    &tuktuk_config_key,
                    name,
                ))
                .await?
                .ok_or_else(|| anyhow::anyhow!("Task queue not found"))?;
            Ok(Some(mapping.task_queue))
        } else {
            Ok(None)
        }
    }
}

impl TaskQueueCmd {
    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::Create {
                queue_authority,
                update_authority,
                capacity,
                name,
            } => {
                let client = opts.client().await?;

                let (key, ix) = tuktuk::task_queue::create(
                    client.rpc_client.as_ref(),
                    client.payer.pubkey(),
                    tuktuk::types::InitializeTaskQueueArgsV0 {
                        capacity: *capacity,
                        crank_reward: 0,
                        name: name.clone(),
                    },
                    *queue_authority,
                    *update_authority,
                )
                .await?;
                let ixs = vec![ix];

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    ixs,
                    &[],
                )
                .await?;

                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Task queue not found: {}", key))?;
                let config_account: TuktukConfigV0 = client
                    .as_ref()
                    .anchor_account(&tuktuk::config_key())
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Tuktuk config account not found"))?;
                let task_queue_ata = spl_associated_token_account::get_associated_token_address(
                    &key,
                    &config_account.network_mint,
                );
                let task_queue_balance = client
                    .rpc_client
                    .get_token_account_balance(&task_queue_ata)
                    .await?;

                print_json(&TaskQueue {
                    pubkey: key,
                    id: task_queue.id,
                    name: name.clone(),
                    capacity: task_queue.capacity,
                    queue_authority: task_queue.queue_authority,
                    update_authority: task_queue.update_authority,
                    default_crank_reward: task_queue.default_crank_reward,
                    balance: task_queue_balance.amount,
                })?;
            }
            Cmd::Get { task_queue } => {
                let client = opts.client().await?;
                let task_queue_key = task_queue.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Must provide task-queue-name, task-queue-id, or task-queue-pubkey"
                    )
                })?;
                let task_queue: TaskQueueV0 = client
                    .rpc_client
                    .anchor_account(&task_queue_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Task queue not found: {}", task_queue_key))?;
                let config_account: TuktukConfigV0 = client
                    .rpc_client
                    .anchor_account(&tuktuk::config_key())
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Tuktuk config account not found"))?;
                let task_queue_ata = spl_associated_token_account::get_associated_token_address(
                    &task_queue_key,
                    &config_account.network_mint,
                );
                let task_queue_balance = client
                    .rpc_client
                    .get_token_account_balance(&task_queue_ata)
                    .await?;
                let serializable = TaskQueue {
                    pubkey: task_queue_key,
                    id: task_queue.id,
                    capacity: task_queue.capacity,
                    queue_authority: task_queue.queue_authority,
                    update_authority: task_queue.update_authority,
                    name: task_queue.name,
                    default_crank_reward: task_queue.default_crank_reward,
                    balance: task_queue_balance.amount,
                };
                print_json(&serializable)?;
            }
            Cmd::Fund { task_queue, amount } => {
                let client = opts.client().await?;
                let task_queue_key = task_queue.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Must provide task-queue-name, task-queue-id, or task-queue-pubkey"
                    )
                })?;
                let tuktuk_config_key = tuktuk::config_key();
                let tuktuk_config: TuktukConfigV0 = client
                    .as_ref()
                    .anchor_account(&tuktuk_config_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Tuktuk config account not found"))?;
                let network_mint = tuktuk_config.network_mint;

                let task_queue_ata = spl_associated_token_account::get_associated_token_address(
                    &task_queue_key,
                    &network_mint,
                );
                let payer_ata = spl_associated_token_account::get_associated_token_address(
                    &client.payer.pubkey(),
                    &network_mint,
                );

                let ix = spl_token::instruction::transfer(
                    &spl_token::id(),
                    &payer_ata,
                    &task_queue_ata,
                    &client.payer.pubkey(),
                    &[],
                    *amount,
                )?;

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    vec![ix],
                    &[],
                )
                .await?;
            }
        }

        Ok(())
    }
}

#[derive(Serialize)]
pub struct TaskQueue {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    pub id: u32,
    pub capacity: u16,
    #[serde(with = "serde_pubkey")]
    pub queue_authority: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub update_authority: Pubkey,
    pub name: String,
    pub default_crank_reward: u64,
    pub balance: String,
}
