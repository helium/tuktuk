use std::collections::HashSet;

use anyhow::anyhow;
use clap::{Args, Subcommand};
use clock::SYSVAR_CLOCK;
use serde::Serialize;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::Transaction};
use tuktuk_program::{types::TriggerV0, TaskQueueV0, TaskV0};
use tuktuk_sdk::prelude::*;

use super::{task_queue::TaskQueueArg, TransactionSource};
use crate::{
    client::send_instructions,
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
    },
    Close {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(short, long)]
        id: u16,
    },
}

impl TaskCmd {
    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::List { task_queue } => {
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

                let clock_acc = client.rpc_client.get_account(&SYSVAR_CLOCK).await?;
                let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock_acc.data)?;
                let now = clock.unix_timestamp;

                let mut json_tasks = Vec::new();
                for (pubkey, maybe_task) in tasks {
                    if let Some(task) = maybe_task {
                        let mut simulation_result = None;
                        if task.trigger.is_active(now) {
                            // Get the run instruction
                            if let Ok(Some(run_ix)) = tuktuk_sdk::compiled_transaction::run_ix(
                                client.as_ref(),
                                pubkey,
                                client.payer.pubkey(),
                                &HashSet::new(),
                            )
                            .await
                            {
                                // Create and simulate the transaction
                                let mut tx = Transaction::new_with_payer(
                                    &run_ix.instructions,
                                    Some(&client.payer.pubkey()),
                                );
                                let recent_blockhash =
                                    client.rpc_client.get_latest_blockhash().await?;
                                tx.message.recent_blockhash = recent_blockhash;
                                tx.sign(&[&client.payer], recent_blockhash);
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
                                    Ok(simulated) => {
                                        simulation_result = Some(SimulationResult {
                                            error: simulated.value.err.map(|e| e.to_string()),
                                            logs: Some(simulated.value.logs.unwrap_or_default()),
                                            compute_units: simulated.value.units_consumed,
                                        });
                                    }
                                    Err(err) => {
                                        simulation_result = Some(SimulationResult {
                                            error: Some(err.to_string()),
                                            logs: None,
                                            compute_units: None,
                                        });
                                    }
                                }
                            }
                        }

                        json_tasks.push(Task {
                            pubkey,
                            id: task.id,
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
            } => {
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();
                let ix = tuktuk::task::dequeue(client.as_ref(), task_queue_pubkey, *index).await?;
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
struct Task {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    #[serde(with = "serde_pubkey")]
    pub rent_refund: Pubkey,
    pub id: u16,
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
