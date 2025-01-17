use std::str::FromStr;

use anchor_client::anchor_lang::InstructionData;
use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signer::Signer, system_instruction::transfer,
};
use tuktuk::cron;
use tuktuk_program::{
    compile_transaction,
    cron::cron::{
        accounts::{CronJobNameMappingV0, CronJobV0, UserCronJobsV0},
        client::{accounts::QueueCronTasksV0, args::QueueCronTasksV0 as QueueCronTasksV0Args},
        types::InitializeCronJobArgsV0,
    },
    types::QueueTaskArgsV0,
    TaskQueueV0, TransactionSourceV0, TriggerV0,
};
use tuktuk_sdk::prelude::*;

use super::task_queue::TaskQueueArg;
use crate::{
    client::{send_instructions, CliClient},
    cmd::Opts,
    result::{anyhow, Result},
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct CronCmd {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    Create {
        #[arg(long)]
        authority: Option<Pubkey>,
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(long)]
        schedule: String,
        #[arg(long)]
        name: String,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=15))]
        free_tasks_per_transaction: u8,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=15))]
        num_tasks_per_queue_call: u8,
        #[arg(long, help = "Initial funding amount in lamports", default_value = "0")]
        funding_amount: u64,
    },
    Get {
        #[command(flatten)]
        cron: CronArg,
    },
    Fund {
        #[command(flatten)]
        cron: CronArg,
        #[arg(long, help = "Amount to fund the cron job with, in lamports")]
        amount: u64,
    },
    Requeue {
        #[command(flatten)]
        cron: CronArg,
    },
    Close {
        #[command(flatten)]
        cron: CronArg,
    },
    List {},
}

#[derive(Debug, Args)]
pub struct CronArg {
    #[arg(long = "cron-name", name = "cron-name")]
    pub name: Option<String>,
    #[arg(long = "cron-id", name = "cron-id")]
    pub id: Option<u32>,
    #[arg(long = "cron-pubkey", name = "cron-pubkey")]
    pub pubkey: Option<String>,
}

impl CronArg {
    pub async fn get_pubkey(&self, client: &CliClient) -> Result<Option<Pubkey>> {
        let authority = client.payer.pubkey();

        if let Some(pubkey) = &self.pubkey {
            // Use the provided pubkey directly
            Ok(Some(Pubkey::from_str(pubkey)?))
        } else if let Some(id) = self.id {
            Ok(Some(tuktuk::cron::cron_job_key(&authority, id)))
        } else if let Some(name) = &self.name {
            let mapping: CronJobNameMappingV0 = client
                .as_ref()
                .anchor_account(&cron::name_mapping_key(&authority, name))
                .await?
                .ok_or_else(|| anyhow::anyhow!("Cron job name mapping not found"))?;
            Ok(Some(mapping.cron_job))
        } else {
            Ok(None)
        }
    }
}

impl CronCmd {
    async fn fund_cron_job_ix(
        client: &CliClient,
        cron_job_key: &Pubkey,
        amount: u64,
    ) -> Result<Instruction> {
        let ix = transfer(&client.payer.pubkey(), cron_job_key, amount);
        Ok(ix)
    }

    async fn requeue_cron_job_ix(client: &CliClient, cron_job_key: &Pubkey) -> Result<Instruction> {
        let cron_job: CronJobV0 = client
            .rpc_client
            .anchor_account(cron_job_key)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Cron job not found: {}", cron_job_key))?;
        let task_queue: TaskQueueV0 = client
            .rpc_client
            .anchor_account(&cron_job.task_queue)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task queue not found: {}", cron_job.task_queue))?;
        let id = task_queue
            .next_available_task_id()
            .ok_or_else(|| anyhow::anyhow!("No available task id"))?;
        let remaining_accounts = (cron_job.current_transaction_id
            ..cron_job.current_transaction_id + cron_job.num_tasks_per_queue_call as u32)
            .map(|i| {
                Pubkey::find_program_address(
                    &[
                        b"cron_job_transaction",
                        cron_job_key.as_ref(),
                        &i.to_le_bytes(),
                    ],
                    &tuktuk_program::cron::ID,
                )
                .0
            })
            .collect::<Vec<Pubkey>>();
        let (queue_tx, _) = compile_transaction(
            vec![Instruction {
                program_id: tuktuk_program::cron::ID,
                accounts: [
                    QueueCronTasksV0 {
                        cron_job: *cron_job_key,
                        task_queue: cron_job.task_queue,
                        task_return_account_1: tuktuk::cron::task_return_account_1_key(
                            cron_job_key,
                        ),
                        task_return_account_2: tuktuk::cron::task_return_account_1_key(
                            cron_job_key,
                        ),
                        system_program: solana_sdk::system_program::ID,
                    }
                    .to_account_metas(None),
                    remaining_accounts
                        .iter()
                        .map(|pubkey| AccountMeta::new_readonly(*pubkey, false))
                        .collect::<Vec<AccountMeta>>(),
                ]
                .concat(),
                data: QueueCronTasksV0Args {}.data(),
            }],
            vec![],
        )?;
        Ok(tuktuk::task::queue(
            client.rpc_client.as_ref(),
            client.payer.pubkey(),
            cron_job.task_queue,
            QueueTaskArgsV0 {
                id,
                trigger: TriggerV0::Now,
                transaction: TransactionSourceV0::CompiledV0(queue_tx),
                crank_reward: None,
                free_tasks: cron_job.num_tasks_per_queue_call + 1,
            },
        )
        .await?
        .1)
    }

    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::Create {
                authority,
                task_queue,
                schedule,
                name,
                free_tasks_per_transaction,
                funding_amount,
                num_tasks_per_queue_call,
            } => {
                let client = opts.client().await?;
                let task_queue_key = task_queue.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Must provide task-queue-name, task-queue-id, or task-queue-pubkey"
                    )
                })?;

                let (key, ix) = tuktuk::cron::create(
                    client.rpc_client.as_ref(),
                    client.payer.pubkey(),
                    InitializeCronJobArgsV0 {
                        name: name.clone(),
                        schedule: schedule.clone(),
                        free_tasks_per_transaction: *free_tasks_per_transaction,
                        num_tasks_per_queue_call: *num_tasks_per_queue_call,
                    },
                    *authority,
                    task_queue_key,
                )
                .await?;

                let fund_ix = Self::fund_cron_job_ix(&client, &key, *funding_amount).await?;

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    vec![fund_ix, ix],
                    &[],
                )
                .await?;

                let cron_job: CronJobV0 = client
                    .as_ref()
                    .anchor_account(&key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Task queue not found: {}", key))?;
                let cron_job_balance = client.rpc_client.get_balance(&key).await?;

                print_json(&CronJob {
                    pubkey: key,
                    id: cron_job.id,
                    name: name.clone(),
                    user_cron_jobs: cron_job.user_cron_jobs,
                    task_queue: cron_job.task_queue,
                    authority: cron_job.authority,
                    free_tasks_per_transaction: cron_job.free_tasks_per_transaction,
                    schedule: cron_job.schedule,
                    current_exec_ts: cron_job.current_exec_ts,
                    current_transaction_id: cron_job.current_transaction_id,
                    next_transaction_id: cron_job.next_transaction_id,
                    balance: cron_job_balance,
                    num_tasks_per_queue_call: *num_tasks_per_queue_call,
                    removed_from_queue: cron_job.removed_from_queue,
                })?;
            }
            Cmd::Get { cron } => {
                let client = opts.client().await?;
                let cron_job_key = cron.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let cron_job: CronJobV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Cron job not found: {}", cron_job_key))?;

                let cron_job_balance = client.rpc_client.get_balance(&cron_job_key).await?;
                let serializable = CronJob {
                    pubkey: cron_job_key,
                    id: cron_job.id,
                    user_cron_jobs: cron_job.user_cron_jobs,
                    task_queue: cron_job.task_queue,
                    authority: cron_job.authority,
                    free_tasks_per_transaction: cron_job.free_tasks_per_transaction,
                    schedule: cron_job.schedule,
                    current_exec_ts: cron_job.current_exec_ts,
                    current_transaction_id: cron_job.current_transaction_id,
                    next_transaction_id: cron_job.next_transaction_id,
                    name: cron_job.name,
                    balance: cron_job_balance,
                    num_tasks_per_queue_call: cron_job.num_tasks_per_queue_call,
                    removed_from_queue: cron_job.removed_from_queue,
                };
                print_json(&serializable)?;
            }
            Cmd::Requeue { cron } => {
                let client = opts.client().await?;
                let cron_job_key = cron.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let cron_job: CronJobV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Cron job not found: {}", cron_job_key))?;

                if cron_job.removed_from_queue {
                    let ix = Self::requeue_cron_job_ix(&client, &cron_job_key).await?;
                    send_instructions(
                        client.rpc_client.clone(),
                        &client.payer,
                        client.opts.ws_url().as_str(),
                        vec![ix],
                        &[],
                    )
                    .await?;
                } else {
                    println!("Cron job does not need to be requeued");
                }
            }
            Cmd::Fund { cron, amount } => {
                let client = opts.client().await?;
                let cron_job_key = cron.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;

                let cron_job: CronJobV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Cron job not found: {}", cron_job_key))?;

                let fund_ix = Self::fund_cron_job_ix(&client, &cron_job_key, *amount).await?;
                let mut ixs = vec![fund_ix];

                if cron_job.removed_from_queue {
                    ixs.push(Self::requeue_cron_job_ix(&client, &cron_job_key).await?);
                }

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    ixs,
                    &[],
                )
                .await?;
            }
            Cmd::Close { cron } => {
                let client: CliClient = opts.client().await?;
                let cron_job_key = cron.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let cron_job: CronJobV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Task queue not found: {}", cron_job_key))?;

                let ix = tuktuk::cron::close(
                    client.as_ref(),
                    cron_job_key,
                    client.payer.pubkey(),
                    Some(cron_job.authority),
                    Some(client.payer.pubkey()),
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
            Cmd::List {} => {
                let client = opts.client().await?;
                let user_cron_jobs_pubkey = cron::user_cron_jobs_key(&client.payer.pubkey());

                let user_cron_jobs: UserCronJobsV0 = client
                    .as_ref()
                    .anchor_account(&user_cron_jobs_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("User cron jobs account not found"))?;
                let cron_job_keys = tuktuk::cron::keys(&client.payer.pubkey(), &user_cron_jobs)?;
                let cron_jobs = client
                    .as_ref()
                    .anchor_accounts::<CronJobV0>(&cron_job_keys)
                    .await?;

                let mut json_cron_jobs = Vec::new();
                for (pubkey, maybe_cron_job) in cron_jobs {
                    if let Some(cron_job) = maybe_cron_job {
                        let cron_job_balance = client.rpc_client.get_balance(&pubkey).await?;
                        json_cron_jobs.push(CronJob {
                            pubkey,
                            id: cron_job.id,
                            user_cron_jobs: cron_job.user_cron_jobs,
                            task_queue: cron_job.task_queue,
                            authority: cron_job.authority,
                            free_tasks_per_transaction: cron_job.free_tasks_per_transaction,
                            schedule: cron_job.schedule,
                            current_exec_ts: cron_job.current_exec_ts,
                            current_transaction_id: cron_job.current_transaction_id,
                            next_transaction_id: cron_job.next_transaction_id,
                            removed_from_queue: cron_job.removed_from_queue,
                            name: cron_job.name,
                            balance: cron_job_balance,
                            num_tasks_per_queue_call: cron_job.num_tasks_per_queue_call,
                        });
                    }
                }
                print_json(&json_cron_jobs)?;
            }
        }

        Ok(())
    }
}

#[derive(Serialize)]
pub struct CronJob {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    pub id: u32,
    #[serde(with = "serde_pubkey")]
    pub user_cron_jobs: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub task_queue: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub authority: Pubkey,
    pub free_tasks_per_transaction: u8,
    pub schedule: String,
    pub name: String,
    pub current_exec_ts: i64,
    pub current_transaction_id: u32,
    pub next_transaction_id: u32,
    pub num_tasks_per_queue_call: u8,
    pub removed_from_queue: bool,
    pub balance: u64,
}
