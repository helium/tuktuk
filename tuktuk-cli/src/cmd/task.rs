use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::Itertools;
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use tuktuk::{types::TriggerV0, TaskQueueV0, TaskV0};
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

                let json_tasks = tasks
                    .into_iter()
                    .filter_map(|(pubkey, maybe_task)| {
                        maybe_task.map(|task| Task {
                            pubkey,
                            id: task.id,
                            trigger: Trigger::from(task.trigger),
                            crank_reward: task.crank_reward,
                            rent_refund: task.rent_refund,
                        })
                    })
                    .collect_vec();
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
