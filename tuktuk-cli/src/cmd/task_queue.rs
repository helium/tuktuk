use std::str::FromStr;

use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signer::Signer, system_instruction::transfer,
};
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
        #[arg(
            long,
            help = "Initial funding amount in lamports. Task queue funding is only required to pay extra rent for tasks that run as a result of other tasks.",
            default_value = "0"
        )]
        funding_amount: u64,
        #[arg(long, help = "Default crank reward in lamports")]
        min_crank_reward: u64,
        #[arg(long, help = "Lookup tables to create")]
        lookup_tables: Option<Vec<Pubkey>>,
        #[arg(
            long,
            help = "Age before a task is considered stale and can be deleted without running the instructions. This is effectively the retention rate for debugging purposes."
        )]
        stale_task_age: u32,
    },
    Update {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(long, help = "Default crank reward in lamports")]
        min_crank_reward: Option<u64>,
        #[arg(long, help = "Lookup tables to create")]
        lookup_tables: Option<Vec<Pubkey>>,
        #[arg(long)]
        update_authority: Option<Pubkey>,
        #[arg(long)]
        capacity: Option<u16>,
        #[arg(
            long,
            help = "Age before a task is considered stale and can be deleted without running the instructions. This is effectively the retention rate for debugging purposes."
        )]
        stale_task_age: Option<u32>,
    },
    Get {
        #[command(flatten)]
        task_queue: TaskQueueArg,
    },
    Fund {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(long, help = "Amount to fund the task queue with, in lamports")]
        amount: u64,
    },
    Close {
        #[command(flatten)]
        task_queue: TaskQueueArg,
    },
    AddQueueAuthority {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(long, help = "Authority to add")]
        queue_authority: Pubkey,
    },
    RemoveQueueAuthority {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(long, help = "Authority to remove")]
        queue_authority: Pubkey,
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
    async fn fund_task_queue_ix(
        client: &CliClient,
        task_queue_key: &Pubkey,
        amount: u64,
    ) -> Result<Instruction> {
        let ix = transfer(&client.payer.pubkey(), task_queue_key, amount);

        Ok(ix)
    }

    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::Create {
                queue_authority,
                update_authority,
                capacity,
                name,
                min_crank_reward,
                funding_amount,
                lookup_tables,
                stale_task_age,
            } => {
                let client = opts.client().await?;

                let (key, ix) = tuktuk::task_queue::create(
                    client.rpc_client.as_ref(),
                    client.payer.pubkey(),
                    tuktuk_program::types::InitializeTaskQueueArgsV0 {
                        capacity: *capacity,
                        min_crank_reward: *min_crank_reward,
                        name: name.clone(),
                        lookup_tables: lookup_tables.clone().unwrap_or_default(),
                        stale_task_age: *stale_task_age,
                    },
                    *update_authority,
                )
                .await?;
                let add_queue_authority_ix = tuktuk::task_queue::add_queue_authority_ix(
                    client.payer.pubkey(),
                    key,
                    queue_authority.unwrap_or(client.payer.pubkey()),
                    update_authority.unwrap_or(client.payer.pubkey()),
                )?;
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
                let fund_ix = Self::fund_task_queue_ix(&client, &key, *funding_amount).await?;

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    vec![fund_ix, ix, add_queue_authority_ix],
                    &[],
                )
                .await?;

                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Task queue not found: {}", key))?;
                let task_queue_balance = client.rpc_client.get_balance(&key).await?;

                print_json(&TaskQueue {
                    pubkey: key,
                    id: task_queue.id,
                    name: name.clone(),
                    capacity: task_queue.capacity,
                    update_authority: task_queue.update_authority,
                    min_crank_reward: task_queue.min_crank_reward,
                    balance: task_queue_balance,
                    stale_task_age: *stale_task_age,
                })?;
            }
            Cmd::Update {
                task_queue,
                min_crank_reward,
                lookup_tables,
                update_authority,
                capacity,
                stale_task_age,
            } => {
                let client = opts.client().await?;
                let task_queue_key = task_queue.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Must provide task-queue-name, task-queue-id, or task-queue-pubkey"
                    )
                })?;
                let ix = tuktuk::task_queue::update(
                    client.rpc_client.as_ref(),
                    client.payer.pubkey(),
                    task_queue_key,
                    tuktuk_program::types::UpdateTaskQueueArgsV0 {
                        capacity: *capacity,
                        min_crank_reward: *min_crank_reward,
                        lookup_tables: lookup_tables.clone(),
                        update_authority: *update_authority,
                        stale_task_age: *stale_task_age,
                    },
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

                let task_queue_balance = client.rpc_client.get_balance(&task_queue_key).await?;
                let serializable = TaskQueue {
                    pubkey: task_queue_key,
                    id: task_queue.id,
                    capacity: task_queue.capacity,
                    update_authority: task_queue.update_authority,
                    name: task_queue.name,
                    min_crank_reward: task_queue.min_crank_reward,
                    balance: task_queue_balance,
                    stale_task_age: task_queue.stale_task_age,
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

                let fund_ix = Self::fund_task_queue_ix(&client, &task_queue_key, *amount).await?;
                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    vec![fund_ix],
                    &[],
                )
                .await?;
            }
            Cmd::AddQueueAuthority {
                task_queue,
                queue_authority,
            } => {
                let client = opts.client().await?;
                let task_queue_key = task_queue.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Must provide task-queue-name, task-queue-id, or task-queue-pubkey"
                    )
                })?;
                let ix = tuktuk::task_queue::add_queue_authority(
                    client.rpc_client.as_ref(),
                    client.payer.pubkey(),
                    task_queue_key,
                    *queue_authority,
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
            Cmd::RemoveQueueAuthority {
                task_queue,
                queue_authority,
            } => {
                let client = opts.client().await?;
                let task_queue_key = task_queue.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Must provide task-queue-name, task-queue-id, or task-queue-pubkey"
                    )
                })?;
                let ix = tuktuk::task_queue::remove_queue_authority(
                    client.rpc_client.as_ref(),
                    client.payer.pubkey(),
                    task_queue_key,
                    *queue_authority,
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
    pub update_authority: Pubkey,
    pub name: String,
    pub min_crank_reward: u64,
    pub balance: u64,
    pub stale_task_age: u32,
}
