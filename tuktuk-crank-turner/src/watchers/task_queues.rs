use std::{collections::HashMap, sync::Arc};

use futures::TryStreamExt;
use tokio::sync::Mutex;
use tokio_graceful_shutdown::{SubsystemBuilder, SubsystemHandle};
use tuktuk::{config_key, task_queue};
use tuktuk_program::{TaskQueueV0, TuktukConfigV0};
use tuktuk_sdk::{client::*, prelude::*};

use super::args::WatcherArgs;
use crate::watchers::tasks::get_and_watch_tasks;

pub async fn get_and_watch_task_queues(
    args: WatcherArgs,
    handle: SubsystemHandle,
    queues_store: Arc<Mutex<HashMap<Pubkey, TaskQueueV0>>>,
) -> anyhow::Result<()> {
    let WatcherArgs {
        rpc_client,
        pubsub_tracker,
        ..
    } = args.clone();
    let config_key = config_key();
    let config = rpc_client
        .anchor_account::<TuktukConfigV0>(&config_key)
        .await?
        .ok_or(anyhow::anyhow!("Tuktuk config not found"))?;

    let task_queue_keys = task_queue::keys(&config_key, &config)?;
    let task_queues = rpc_client
        .anchor_accounts::<TaskQueueV0>(&task_queue_keys)
        .await?;

    let (stream, _) = task_queue::on_new(
        rpc_client.as_ref(),
        pubsub_tracker.as_ref(),
        &config_key,
        &config,
    )
    .await?;

    for (task_queue_key, maybe_task_queue) in task_queues {
        let args = args.clone();
        if let Some(task_queue) = maybe_task_queue {
            if task_queue.min_crank_reward >= args.min_crank_fee {
                handle.start(SubsystemBuilder::new("task-queue-watcher", {
                    let queues_store = queues_store.clone();
                    move |handle| {
                        get_and_watch_tasks(task_queue_key, task_queue, args, handle, queues_store)
                    }
                }));
            }
        }
    }

    let stream_fut = stream
        .map_err(|e| anyhow::anyhow!("Error in auctions stream: {}", e))
        .try_for_each(|update| {
            for (task_queue_key, task_queue_account) in update.task_queues {
                if let Some(task_queue) = task_queue_account {
                    let args = args.clone();
                    handle.start(SubsystemBuilder::new("task-queue-watcher", {
                        let queues_store = queues_store.clone();
                        move |handle| {
                            get_and_watch_tasks(
                                task_queue_key,
                                task_queue,
                                args,
                                handle,
                                queues_store,
                            )
                        }
                    }));
                }
            }
            async move { anyhow::Ok(()) }
        });

    tokio::select! {
        res = stream_fut => res,
        _ = handle.on_shutdown_requested() => anyhow::Ok(()),
    }
}
