use std::str::FromStr;

use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use tuktuk::task_queue_name_mapping_key;
use tuktuk_program::{TaskQueueV0, TuktukConfigV0};
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
        #[arg(long, help = "Initial funding amount in bones")]
        funding_amount: u64,
        #[arg(long, help = "Default crank reward in bones")]
        crank_reward: u64,
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
    Close {
        #[command(flatten)]
        task_queue: TaskQueueArg,
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
            let mapping: tuktuk_program::TaskQueueNameMappingV0 = client
                .as_ref()
                .anchor_account(&task_queue_name_mapping_key(&tuktuk_config_key, name))
                .await?
                .ok_or_else(|| anyhow::anyhow!("Task queue not found"))?;
            Ok(Some(mapping.task_queue))
        } else {
            Ok(None)
        }
    }
}

impl TaskQueueCmd {
    async fn fund_task_queue(client: &CliClient, task_queue_key: &Pubkey, amount: u64) -> Result {
        let tuktuk_config: TuktukConfigV0 = client
            .as_ref()
            .anchor_account(&tuktuk::config_key())
            .await?
            .ok_or_else(|| anyhow::anyhow!("Tuktuk config account not found"))?;

        let task_queue_ata = spl_associated_token_account::get_associated_token_address(
            task_queue_key,
            &tuktuk_config.network_mint,
        );
        let payer_ata = spl_associated_token_account::get_associated_token_address(
            &client.payer.pubkey(),
            &tuktuk_config.network_mint,
        );

        let init_idemp =
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &client.payer.pubkey(),
                task_queue_key,
                &tuktuk_config.network_mint,
                &spl_token::id(),
            );

        let ix = spl_token::instruction::transfer(
            &spl_token::id(),
            &payer_ata,
            &task_queue_ata,
            &client.payer.pubkey(),
            &[],
            amount,
        )?;

        send_instructions(
            client.rpc_client.clone(),
            &client.payer,
            client.opts.ws_url().as_str(),
            vec![init_idemp, ix],
            &[],
        )
        .await
    }

    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::Create {
                queue_authority,
                update_authority,
                capacity,
                name,
                crank_reward,
                funding_amount,
            } => {
                let client = opts.client().await?;

                let (key, ix) = tuktuk::task_queue::create(
                    client.rpc_client.as_ref(),
                    client.payer.pubkey(),
                    tuktuk_program::types::InitializeTaskQueueArgsV0 {
                        capacity: *capacity,
                        crank_reward: *crank_reward,
                        name: name.clone(),
                    },
                    *queue_authority,
                    *update_authority,
                )
                .await?;

                // Fund if amount specified
                let config: TuktukConfigV0 = client
                    .as_ref()
                    .anchor_account(&tuktuk::config_key())
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Tuktuk config account not found"))?;
                if *funding_amount < config.min_deposit {
                    return Err(anyhow::anyhow!(
                        "Funding amount must be greater than the minimum deposit: {}",
                        config.min_deposit
                    ));
                }
                Self::fund_task_queue(&client, &key, config.min_deposit).await?;

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    vec![ix],
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

                Self::fund_task_queue(&client, &task_queue_key, *amount).await?;
            }
            Cmd::Close { task_queue } => {
                let client = opts.client().await?;
                let task_queue_key = task_queue.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Must provide task-queue-name, task-queue-id, or task-queue-pubkey"
                    )
                })?;

                let ix = tuktuk::task_queue::close(
                    client.rpc_client.as_ref(),
                    task_queue_key,
                    client.payer.pubkey(),
                    client.payer.pubkey(),
                )
                .await?;
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
