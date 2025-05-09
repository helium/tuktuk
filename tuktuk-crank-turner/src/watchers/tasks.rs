use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::TryStreamExt;
use solana_sdk::pubkey::Pubkey;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{debug, info};
use tuktuk::task;
use tuktuk_program::{types::TriggerV0, TaskQueueV0, TaskV0};
use tuktuk_sdk::{prelude::*, watcher::UpdateType};

use super::args::WatcherArgs;
use crate::{
    cache::TaskQueuesSender,
    metrics::{UPDATE_LAG, UPDATE_SOURCE},
    task_queue::TimedTask,
};

pub async fn get_and_watch_tasks(
    task_queue_key: Pubkey,
    task_queue_account: TaskQueueV0,
    args: WatcherArgs,
    handle: SubsystemHandle,
    task_queues: TaskQueuesSender,
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
    let (stream, unsub) = task::on_new(
        rpc_client.as_ref(),
        pubsub_tracker.as_ref(),
        &task_queue_key,
        &task_queue_account,
    )
    .await?;

    async fn save_task_queue(
        task_queue_key: Pubkey,
        task_queue: TaskQueueV0,
        task_queues: TaskQueuesSender,
    ) {
        task_queues
            .update_task_queue(task_queue_key, task_queue)
            .await;
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
                    is_cleanup_task: false,
                    profitability_delayed: false,
                    cached_result: None,
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
                    is_cleanup_task: false,
                    profitability_delayed: false,
                    cached_result: None,
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
            let updated = !update.tasks.is_empty() || !update.removed.is_empty();

            if updated {
                match update.update_type {
                    UpdateType::Poll => UPDATE_SOURCE
                        .with_label_values(&[task_queue_account.name.as_str(), "poll"])
                        .inc(),
                    UpdateType::Websocket => UPDATE_SOURCE
                        .with_label_values(&[task_queue_account.name.as_str(), "websocket"])
                        .inc(),
                }
            }

            async move {
                save_task_queue(task_queue_key, task_queue_account.clone(), task_queues).await;
                let now = *now.borrow();
                if updated {
                    UPDATE_LAG
                        .with_label_values(&[task_queue_account.name.as_str(), "solana_clock"])
                        .set(now as i64 - task_queue_account.updated_at);
                    let unix_now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_else(|_| Duration::from_secs(0))
                        .as_secs();
                    UPDATE_LAG
                        .with_label_values(&[task_queue_account.name.as_str(), "realtime_clock"])
                        .set(unix_now as i64 - task_queue_account.updated_at);
                }

                for removed in update.removed {
                    debug!(?removed, "removing task");
                    task_queue
                        .remove_task(removed)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to remove task: {}", e))?;
                }
                for (task_key, account) in update.tasks {
                    debug!(?task_key, "received task, is_some: {}", account.is_some());
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
                                    is_cleanup_task: false,
                                    profitability_delayed: false,
                                    cached_result: None,
                                },
                                TriggerV0::Timestamp(ts) => TimedTask {
                                    task_time: ts as u64,
                                    task_queue_name: task_queue_account.name.clone(),
                                    task: t.clone(),
                                    task_key,
                                    max_retries: args.max_retries,
                                    task_queue_key,
                                    cached_result: None,
                                    total_retries: 0,
                                    in_flight_task_ids: vec![],
                                    is_cleanup_task: false,
                                    profitability_delayed: false,
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
        res = stream_fut => {
            if res.is_err() {
                unsub().await;
            }
            res
        },
        _ = handle.on_shutdown_requested() => {
            unsub().await;
            anyhow::Ok(())
        }
    }
}
