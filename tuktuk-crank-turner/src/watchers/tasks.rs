use std::{collections::HashMap, sync::Arc};

use futures::TryStreamExt;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::Mutex;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;
use tuktuk::task;
use tuktuk_program::{types::TriggerV0, TaskQueueV0, TaskV0};
use tuktuk_sdk::prelude::*;

use super::args::WatcherArgs;
use crate::task_queue::TimedTask;

pub async fn get_and_watch_tasks(
    task_queue_key: Pubkey,
    task_queue_account: TaskQueueV0,
    args: WatcherArgs,
    handle: SubsystemHandle,
    task_queues: Arc<Mutex<HashMap<Pubkey, TaskQueueV0>>>,
) -> anyhow::Result<()> {
    info!(?task_queue_key, "watching tasks for queue");
    let WatcherArgs {
        rpc_client,
        pubsub_tracker,
        task_queue,
        now,
        ..
    } = args;
    let task_keys = task::keys(&task_queue_key, &task_queue_account)?;
    let tasks = rpc_client.anchor_accounts::<TaskV0>(&task_keys).await?;

    let task_queue = task_queue.clone();
    let now = now.clone();
    let rpc_client = rpc_client.clone();
    let (stream, _) = task::on_new(
        rpc_client.as_ref(),
        pubsub_tracker.as_ref(),
        &task_queue_key,
        &task_queue_account,
    )
    .await?;

    async fn save_task_queue(
        task_queue_key: Pubkey,
        task_queue: TaskQueueV0,
        task_queues: Arc<Mutex<HashMap<Pubkey, TaskQueueV0>>>,
    ) {
        let mut lock = task_queues.lock().await;
        lock.insert(task_queue_key, task_queue);
    }

    save_task_queue(
        task_queue_key,
        task_queue_account.clone(),
        task_queues.clone(),
    )
    .await;

    for (task_key, account) in tasks {
        let task = match account {
            Some(t) if t.crank_reward >= args.min_crank_fee => match t.trigger {
                TriggerV0::Now => TimedTask {
                    task_queue_name: task_queue_account.name.clone(),
                    task: t.clone(),
                    task_time: *now.borrow(),
                    task_key,
                    total_retries: 0,
                    max_retries: args.max_retries,
                    task_queue_key,
                    in_flight_task_ids: vec![],
                },
                TriggerV0::Timestamp(ts) => TimedTask {
                    task_queue_name: task_queue_account.name.clone(),
                    task: t.clone(),
                    task_time: ts as u64,
                    task_key,
                    total_retries: 0,
                    max_retries: args.max_retries,
                    task_queue_key,
                    in_flight_task_ids: vec![],
                },
            },
            _ => continue,
        };
        task_queue
            .add_task(task)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to add task: {}", e))?;
    }

    let stream_fut = stream
        .map_err(|e| anyhow::anyhow!("Error in queue nodes stream: {}", e))
        .try_for_each(|update| {
            let task_queue = task_queue.clone();
            let now = now.clone();
            let task_queue_account = update.task_queue;
            let task_queues = task_queues.clone();

            async move {
                save_task_queue(task_queue_key, task_queue_account.clone(), task_queues).await;
                let now = *now.borrow();
                for (task_key, account) in update.tasks {
                    match &account {
                        Some(t) if t.crank_reward >= args.min_crank_fee => {
                            let task = match t.trigger {
                                TriggerV0::Now => TimedTask {
                                    task_queue_name: task_queue_account.name.clone(),
                                    task: t.clone(),
                                    task_time: now,
                                    task_key,
                                    total_retries: 0,
                                    max_retries: args.max_retries,
                                    task_queue_key,
                                    in_flight_task_ids: vec![],
                                },
                                TriggerV0::Timestamp(ts) => TimedTask {
                                    task_time: ts as u64,
                                    task_queue_name: task_queue_account.name.clone(),
                                    task: t.clone(),
                                    task_key,
                                    total_retries: 0,
                                    max_retries: args.max_retries,
                                    task_queue_key,
                                    in_flight_task_ids: vec![],
                                },
                            };
                            task_queue
                                .add_task(task)
                                .await
                                .map_err(|e| anyhow::anyhow!("Failed to add task: {}", e))?;
                        }
                        _ => (),
                    }
                }

                Ok(())
            }
        });

    tokio::select! {
        res = stream_fut => res.map_err(anyhow::Error::from),
        _ = handle.on_shutdown_requested() => anyhow::Ok(()),
    }
}
