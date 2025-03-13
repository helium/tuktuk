use std::collections::HashSet;

use anyhow::anyhow;
use clap::{Args, Subcommand};
use clock::SYSVAR_CLOCK;
use serde::Serialize;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    commitment_config::CommitmentLevel,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signer::Signer,
    transaction::VersionedTransaction,
};
use solana_transaction_utils::{
    pack::pack_instructions_into_transactions, priority_fee::auto_compute_limit_and_price,
};
use tuktuk_program::{types::TriggerV0, TaskQueueV0, TaskV0};
use tuktuk_sdk::prelude::*;

use super::{task_queue::TaskQueueArg, TransactionSource};
use crate::{
    client::{send_instructions, CliClient},
    cmd::Opts,
    result::Result,
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct TaskCmd {
    #[arg(long, default_value = "false")]
    pub verbose: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    List {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        // Description prefix for the task to filter by
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "false")]
        skip_simulate: bool,
    },
    Run {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(short, long)]
        id: Option<u16>,
        // Description prefix to run by
        #[arg(long)]
        description: Option<String>,
        #[arg(short, long, default_value = "false")]
        skip_preflight: bool,
    },
    Close {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(short, long)]
        id: Option<u16>,
        // Description prefix to close by
        #[arg(long)]
        description: Option<String>,
        #[arg(
            long,
            default_value = "false",
            help = "Close tasks that fail simulation"
        )]
        failed: bool,
    },
}

async fn simulate_task(client: &CliClient, task_key: Pubkey) -> Result<Option<SimulationResult>> {
    // Get the run instruction
    let run_ix = tuktuk_sdk::compiled_transaction::run_ix(
        client.as_ref(),
        task_key,
        client.payer.pubkey(),
        &HashSet::new(),
    )
    .await?;

    if let Some(run_ix) = run_ix {
        // Create and simulate the transaction
        let mut updated_instructions = vec![
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(1900000),
        ];
        updated_instructions.extend(run_ix.instructions.clone());
        let recent_blockhash = client.rpc_client.get_latest_blockhash().await?;
        let message = VersionedMessage::V0(v0::Message::try_compile(
            &client.payer.pubkey(),
            &updated_instructions,
            &run_ix.lookup_tables,
            recent_blockhash,
        )?);
        let tx = VersionedTransaction::try_new(message, &[&client.payer])?;
        let sim_result = client
            .rpc_client
            .simulate_transaction_with_config(
                &tx,
                RpcSimulateTransactionConfig {
                    commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
                    sig_verify: true,
                    ..Default::default()
                },
            )
            .await;

        match sim_result {
            Ok(simulated) => Ok(Some(SimulationResult {
                error: simulated.value.err.map(|e| e.to_string()),
                logs: Some(simulated.value.logs.unwrap_or_default()),
                compute_units: simulated.value.units_consumed,
            })),
            Err(err) => Ok(Some(SimulationResult {
                error: Some(err.to_string()),
                logs: None,
                compute_units: None,
            })),
        }
    } else {
        Ok(None)
    }
}

impl TaskCmd {
    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::List {
                task_queue,
                description,
                skip_simulate,
            } => {
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();

                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Topic account not found"))?;
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let tasks = client
                    .as_ref()
                    .anchor_accounts::<TaskV0>(&task_keys)
                    .await?;
                let filtered_tasks = tasks.into_iter().filter(|(_, task)| {
                    if let Some(task) = task {
                        if let Some(description) = description {
                            return task.description.starts_with(description);
                        }
                    }
                    true
                });

                let clock_acc = client.rpc_client.get_account(&SYSVAR_CLOCK).await?;
                let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock_acc.data)?;
                let now = clock.unix_timestamp;

                let mut json_tasks = Vec::new();
                for (pubkey, maybe_task) in filtered_tasks {
                    if let Some(task) = maybe_task {
                        let mut simulation_result = None;
                        if !*skip_simulate && task.trigger.is_active(now) {
                            simulation_result = simulate_task(&client, pubkey).await?;
                        }

                        json_tasks.push(Task {
                            pubkey,
                            id: task.id,
                            description: task.description,
                            trigger: Trigger::from(task.trigger),
                            crank_reward: task.crank_reward,
                            rent_refund: task.rent_refund,
                            simulation_result,
                            transaction: if self.verbose {
                                Some(TransactionSource::from(task.transaction.clone()))
                            } else {
                                None
                            },
                        });
                    }
                }
                print_json(&json_tasks)?;
            }
            Cmd::Close {
                task_queue,
                id: index,
                description,
                failed,
            } => {
                if index.is_none() && description.is_none() {
                    return Err(anyhow!("Either id or description must be provided"));
                }
                if index.is_some() && description.is_some() {
                    return Err(anyhow!("Only one of id or description can be provided"));
                }
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();
                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Task queue account not found"))?;
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let tasks = if let Some(index) = index {
                    let task_key = tuktuk::task::key(&task_queue_pubkey, *index);
                    let task = client
                        .as_ref()
                        .anchor_account::<TaskV0>(&task_key)
                        .await?
                        .ok_or_else(|| anyhow!("Task not found"))?;
                    vec![(task_key, task)]
                } else if let Some(description) = description {
                    let tasks = client
                        .as_ref()
                        .anchor_accounts::<TaskV0>(&task_keys)
                        .await?;
                    tasks
                        .into_iter()
                        .filter(|(_, task)| {
                            if let Some(task) = task {
                                return task.description.starts_with(description);
                            }
                            false
                        })
                        .map(|(p, task)| (p, task.unwrap().clone()))
                        .collect()
                } else {
                    vec![]
                };

                let mut seen_ids = HashSet::new();
                let mut to_close = Vec::new();

                // If failed flag is set, simulate each task first
                for (pubkey, task) in &tasks {
                    if seen_ids.insert(task.id) {
                        if *failed {
                            if let Some(sim_result) = simulate_task(&client, *pubkey).await? {
                                if sim_result.error.is_some() {
                                    to_close.push(task.clone());
                                }
                            }
                        } else {
                            to_close.push(task.clone());
                        }
                    }
                }

                let ixs = to_close
                    .into_iter()
                    .map(|task| {
                        tuktuk::task::dequeue_ix(
                            task_queue_pubkey,
                            client.payer.pubkey(),
                            task.rent_refund,
                            task.id,
                        )
                        .map_err(|e| anyhow!("Failed to dequeue task: {}", e))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let groups = pack_instructions_into_transactions(
                    ixs.into_iter().map(|ix| vec![ix]).collect(),
                    &client.payer,
                    None,
                )?;

                for mut to_send in groups {
                    // Remove compute budget ixs
                    to_send.instructions.remove(0);
                    to_send.instructions.remove(0);
                    send_instructions(
                        client.rpc_client.clone(),
                        &client.payer,
                        client.opts.ws_url().as_str(),
                        to_send.instructions,
                        &[],
                    )
                    .await?;
                }
            }
            Cmd::Run {
                task_queue,
                id,
                skip_preflight,
                description,
            } => {
                if id.is_none() && description.is_none() {
                    return Err(anyhow!("Either id or description must be provided"));
                }
                if id.is_some() && description.is_some() {
                    return Err(anyhow!("Only one of id or description can be provided"));
                }
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();
                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Task queue account not found"))?;
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let tasks = if let Some(id) = id {
                    let task_key = tuktuk::task::key(&task_queue_pubkey, *id);
                    let task = client
                        .as_ref()
                        .anchor_account::<TaskV0>(&task_key)
                        .await?
                        .ok_or_else(|| anyhow!("Task not found"))?;
                    vec![(task_key, task)]
                } else if let Some(description) = description {
                    let tasks = client
                        .as_ref()
                        .anchor_accounts::<TaskV0>(&task_keys)
                        .await?;
                    tasks
                        .into_iter()
                        .filter(|(_, task)| {
                            if let Some(task) = task {
                                return task.description.starts_with(description);
                            }
                            false
                        })
                        .map(|(p, task)| (p, task.unwrap().clone()))
                        .collect()
                } else {
                    vec![]
                };
                for (task_key, _) in tasks {
                    let run_ix_result = tuktuk_sdk::compiled_transaction::run_ix(
                        client.as_ref(),
                        task_key,
                        client.payer.pubkey(),
                        &HashSet::new(),
                    )
                    .await;
                    match run_ix_result {
                        Ok(Some(run_ix)) => {
                            let blockhash = client.rpc_client.get_latest_blockhash().await?;
                            let (computed, _) = auto_compute_limit_and_price(
                                &client.rpc_client,
                                run_ix.instructions,
                                1.2,
                                &client.payer.pubkey(),
                                Some(blockhash),
                                Some(run_ix.lookup_tables.clone()),
                            )
                            .await
                            .unwrap();

                            let recent_blockhash = client.rpc_client.get_latest_blockhash().await?;
                            let message = VersionedMessage::V0(v0::Message::try_compile(
                                &client.payer.pubkey(),
                                &computed,
                                &run_ix.lookup_tables,
                                recent_blockhash,
                            )?);
                            let tx = VersionedTransaction::try_new(message, &[&client.payer])?;
                            let txid = client
                                .rpc_client
                                .send_transaction_with_config(
                                    &tx,
                                    solana_client::rpc_config::RpcSendTransactionConfig {
                                        skip_preflight: *skip_preflight,
                                        preflight_commitment: Some(CommitmentLevel::Confirmed),
                                        ..Default::default()
                                    },
                                )
                                .await?;

                            println!("Tx sent: {}", txid);
                        }
                        Err(e) => {
                            println!("Error running task: {}", e);
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct Task {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    pub id: u16,
    pub description: String,
    #[serde(with = "serde_pubkey")]
    pub rent_refund: Pubkey,
    pub trigger: Trigger,
    pub crank_reward: u64,
    pub simulation_result: Option<SimulationResult>,
    pub transaction: Option<TransactionSource>,
}

#[derive(Serialize)]
struct SimulationResult {
    pub error: Option<String>,
    pub logs: Option<Vec<String>>,
    pub compute_units: Option<u64>,
}

#[derive(Serialize)]
enum Trigger {
    Now,
    Timestamp(i64),
}

impl From<TriggerV0> for Trigger {
    fn from(trigger: TriggerV0) -> Self {
        match trigger {
            TriggerV0::Now => Trigger::Now,
            TriggerV0::Timestamp(ts) => Trigger::Timestamp(ts),
        }
    }
}
