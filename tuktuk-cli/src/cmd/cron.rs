use std::str::FromStr;

use clap::{Args, Subcommand};
use serde::Serialize;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signer::Signer, system_instruction::transfer,
};
use tuktuk::cron;
use tuktuk_program::{
    cron::{
        accounts::{CronJobNameMappingV0, CronJobV0, UserCronJobsV0},
        types::InitializeCronJobArgsV0,
    },
    tuktuk::{accounts::TaskV0, types::TransactionSourceV0},
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
        #[arg(
            long,
            help = "Send the requeue even if the cron job doesn't think it is removed from \
                    queue. The program still refuses while the task it recorded is live.",
            default_value = "false"
        )]
        force: bool,
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

/// What the program's requeue gate will make of the task a cron job recorded.
enum RequeueState {
    /// Nothing holds the recorded address, so the gate admits a requeue.
    Ready,
    /// A task that is not this job's schedule holds it. Task ids are recycled as tasks close, so
    /// the address has to be freed before the gate admits a requeue.
    Occupied {
        task: Pubkey,
        id: u16,
        rent_refund: Pubkey,
    },
    /// The job's own schedule task is live, so it is already on the queue.
    Live,
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

    /// The same question the program's requeue gate asks: is the schedule task this cron job
    /// recorded gone? `Pubkey::default()` is the system program, which exists, so it is answered
    /// before any account is fetched. `removed_from_queue` is only set on the two lamport
    /// shortfall paths, so it does not answer this on its own.
    ///
    /// Task ids are reused as tasks close, so a task at the recorded address is not necessarily
    /// this job's. A schedule task names the cron job it advances, which is what separates the two.
    async fn requeue_state(
        client: &CliClient,
        cron_job_key: &Pubkey,
        cron_job: &CronJobV0,
    ) -> Result<RequeueState> {
        if cron_job.next_schedule_task == Pubkey::default() {
            return Ok(RequeueState::Ready);
        }
        let task: Option<TaskV0> = client
            .rpc_client
            .anchor_account(&cron_job.next_schedule_task)
            .await?;
        let Some(task) = task else {
            return Ok(RequeueState::Ready);
        };
        if Self::advances(&task, cron_job_key) {
            return Ok(RequeueState::Live);
        }

        Ok(RequeueState::Occupied {
            task: cron_job.next_schedule_task,
            id: task.id,
            rent_refund: task.rent_refund,
        })
    }

    /// Whether a task carries the given cron job's schedule. The cron program compiles that task
    /// itself and names the job in it, so only a compiled transaction can be one.
    fn advances(task: &TaskV0, cron_job_key: &Pubkey) -> bool {
        match &task.transaction {
            TransactionSourceV0::CompiledV0(transaction) => {
                transaction.accounts.contains(cron_job_key)
            }
            TransactionSourceV0::RemoteV0 { .. } => false,
        }
    }

    /// The instructions that put a cron job back on the queue. The program admits a requeue only
    /// while nothing holds the address the job recorded, so when an unrelated task holds it that
    /// task is closed first, in the same transaction: anchor's `close` assigns the account to the
    /// system program and truncates it, so the requeue that follows reads it as empty, and no one
    /// can take the id in between.
    async fn requeue_ixs(
        client: &CliClient,
        cron_job_key: &Pubkey,
        cron_job: &CronJobV0,
        state: RequeueState,
    ) -> Result<Vec<Instruction>> {
        let mut ixs = Vec::new();

        if let RequeueState::Occupied {
            task,
            id,
            rent_refund,
        } = state
        {
            // `dequeue_ix` derives the task from the id, so the id has to be the one that derives
            // the address actually recorded. Anything else would close a different task.
            let derived = tuktuk::task::key(&cron_job.task_queue, id);
            if derived != task {
                return Err(anyhow!(
                    "task {task} recorded by cron job {cron_job_key} is not task {id} of queue {}",
                    cron_job.task_queue
                ));
            }
            println!("Freeing {task}, which holds the address this cron job recorded");
            ixs.push(tuktuk::task::dequeue_ix(
                cron_job.task_queue,
                client.payer.pubkey(),
                rent_refund,
                id,
            )?);
        }

        ixs.push(Self::requeue_cron_job_ix(client, cron_job_key).await?);

        Ok(ixs)
    }

    async fn requeue_cron_job_ix(client: &CliClient, cron_job_key: &Pubkey) -> Result<Instruction> {
        Ok(tuktuk::cron::requeue(
            client.rpc_client.as_ref(),
            client.payer.pubkey(),
            client.payer.pubkey(),
            *cron_job_key,
        )
        .await?)
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
                    &[fund_ix, ix],
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
                    next_schedule_task: cron_job.next_schedule_task,
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
                    next_schedule_task: cron_job.next_schedule_task,
                };
                print_json(&serializable)?;
            }
            Cmd::Requeue { cron, force } => {
                let client = opts.client().await?;
                let cron_job_key = cron.get_pubkey(&client).await?.ok_or_else(|| {
                    anyhow::anyhow!("Must provide cron-name, cron-id, or cron-pubkey")
                })?;
                let cron_job: CronJobV0 = client
                    .rpc_client
                    .anchor_account(&cron_job_key)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Cron job not found: {}", cron_job_key))?;

                let state = Self::requeue_state(&client, &cron_job_key, &cron_job).await?;
                if !*force && matches!(state, RequeueState::Live) {
                    println!("Cron job does not need to be requeued");
                } else {
                    let ixs = Self::requeue_ixs(&client, &cron_job_key, &cron_job, state).await?;
                    send_instructions(
                        client.rpc_client.clone(),
                        &client.payer,
                        client.opts.ws_url().as_str(),
                        &ixs,
                        &[],
                    )
                    .await?;
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

                let state = Self::requeue_state(&client, &cron_job_key, &cron_job).await?;
                if !matches!(state, RequeueState::Live) {
                    ixs.extend(Self::requeue_ixs(&client, &cron_job_key, &cron_job, state).await?);
                }

                send_instructions(
                    client.rpc_client.clone(),
                    &client.payer,
                    client.opts.ws_url().as_str(),
                    &ixs,
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
                    &[ix],
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
                            next_schedule_task: cron_job.next_schedule_task,
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
    #[serde(with = "serde_pubkey")]
    pub next_schedule_task: Pubkey,
}
