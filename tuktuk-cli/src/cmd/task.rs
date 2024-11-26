use anyhow::anyhow;
use clap::{Args, Subcommand};
use clock::SYSVAR_CLOCK;
use serde::Serialize;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::Transaction};
use tuktuk::{types::TriggerV0, TaskQueueV0, TaskV0, TuktukConfigV0};
use tuktuk_sdk::prelude::*;

use super::task_queue::TaskQueueArg;
use crate::{
    cmd::Opts,
    result::Result,
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct TaskCmd {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    List {
        #[command(flatten)]
        task_queue: TaskQueueArg,
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
                let tuktuk_config: TuktukConfigV0 = client
                    .as_ref()
                    .anchor_account(&tuktuk::config_key())
                    .await?
                    .ok_or_else(|| anyhow!("Tuktuk config account not found"))?;

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
                            )
                            .await
                            {
                                // Create and simulate the transaction
                                // Ensure they have an associated token account for hnt
                                let init_idemp = spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                                    &client.payer.pubkey(),
                                    &client.payer.pubkey(),
                                    &tuktuk_config.network_mint,
                                    &spl_token::id(),
                                );
                                let mut tx = Transaction::new_with_payer(
                                    &[init_idemp, run_ix],
                                    Some(&client.payer.pubkey()),
                                );
                                let recent_blockhash =
                                    client.rpc_client.get_latest_blockhash().await?;
                                tx.message.recent_blockhash = recent_blockhash;
                                let sim_result =
                                    client.rpc_client.simulate_transaction(&tx).await?;

                                simulation_result = Some(SimulationResult {
                                    error: sim_result.value.err.map(|e| e.to_string()),
                                    logs: Some(sim_result.value.logs.unwrap_or_default()),
                                    compute_units: sim_result.value.units_consumed,
                                });
                            }
                        }

                        json_tasks.push(Task {
                            pubkey,
                            id: task.id,
                            trigger: Trigger::from(task.trigger),
                            crank_reward: task.crank_reward,
                            rent_refund: task.rent_refund,
                            simulation_result,
                        });
                    }
                }
                print_json(&json_tasks)?;
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
