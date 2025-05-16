use std::{cmp::max, sync::Arc};

use futures::{Stream, StreamExt, TryStreamExt};
use solana_sdk::{instruction::InstructionError, signer::Signer, transaction::TransactionError};
use solana_transaction_utils::{error::Error as TransactionQueueError, queue::TransactionTask};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{debug, info};
use tuktuk_program::TaskQueueV0;
use tuktuk_sdk::compiled_transaction::{
    next_available_task_ids_excluding_in_progress, run_ix_with_free_tasks,
};

use crate::{
    metrics::{TASKS_COMPLETED, TASKS_FAILED, TASKS_IN_PROGRESS, TASK_IDS_RESERVED},
    task_context::TaskContext,
    task_queue::TimedTask,
};

impl TimedTask {
    pub async fn get_task_queue(
        &self,
        ctx: Arc<TaskContext>,
    ) -> Result<TaskQueueV0, tuktuk_sdk::error::Error> {
        ctx.task_queues_client
            .get_task_queue(self.task_queue_key)
            .await
            .ok()
            .flatten()
            .ok_or(tuktuk_sdk::error::Error::AccountNotFound)
    }

    pub async fn get_available_task_ids(
        &self,
        ctx: Arc<TaskContext>,
    ) -> Result<Vec<u16>, anyhow::Error> {
        let task_queue = self.get_task_queue(ctx.clone()).await?;
        let task_ids = ctx
            .task_state_client
            .get_in_progress_tasks(self.task_queue_key)
            .await
            .map_err(|_| anyhow::anyhow!("task state channel closed"))?;

        TASKS_IN_PROGRESS
            .with_label_values(&[self.task_queue_name.as_str()])
            .inc();
        TASK_IDS_RESERVED
            .with_label_values(&[self.task_queue_name.as_str()])
            .set(task_ids.len() as i64);

        let next_available = next_available_task_ids_excluding_in_progress(
            task_queue.capacity,
            &task_queue.task_bitmap,
            self.task.free_tasks,
            &task_ids,
            rand::random_range(0..task_queue.task_bitmap.len()),
        )?;
        ctx.task_state_client
            .add_in_progress_tasks(
                self.task_queue_key,
                next_available.iter().cloned().collect(),
            )
            .await;
        Ok(next_available)
    }

    async fn handle_ix_err(
        &self,
        ctx: Arc<TaskContext>,
        err: tuktuk_sdk::error::Error,
    ) -> anyhow::Result<()> {
        info!(?self.task_key, ?self.task_time, ?err, "getting instructions failed");
        let ctx = ctx.clone();
        match err {
            tuktuk_sdk::error::Error::AccountNotFound => {
                info!("lookup table accounts, removing from queue");
                self.handle_completion(ctx, None, 0).await
            }
            _ => {
                self.handle_completion(
                    ctx,
                    Some(TransactionQueueError::RawSimulatedTransactionError(
                        format!("Failed to get instructions: {err:?}"),
                    )),
                    0,
                )
                .await
            }
        }
    }

    pub async fn process(&mut self, ctx: Arc<TaskContext>) -> anyhow::Result<()> {
        let TaskContext {
            payer,
            tx_sender,
            task_queue,
            profitability,
            ..
        } = &*ctx;

        // Maybe delay the task by 5-10 seconds if it's not profitable
        if self.total_retries == 0 && !self.profitability_delayed {
            let now = *ctx.now_rx.borrow();
            let should_delay = profitability.should_delay(&self.task_queue_name).await;
            if should_delay {
                let delay = rand::random_range(5..15);
                debug!(?self.task_key, ?delay, "task is from unprofitable queue, delaying");
                task_queue
                    .add_task(TimedTask {
                        profitability_delayed: true,
                        task_time: max(now, self.task_time) + delay,
                        ..self.clone()
                    })
                    .await?;
                return Ok(());
            }
        }

        let task_queue = self.get_task_queue(ctx.clone()).await?;

        let lookup_tables = ctx
            .lookup_tables_client
            .get_lookup_tables(task_queue.lookup_tables)
            .await
            .map_err(|_| anyhow::anyhow!("lookup tables channel closed"))?;
        let next_available = self.get_available_task_ids(ctx.clone()).await?;
        self.in_flight_task_ids = next_available.clone();

        let maybe_run_ix = if let Some(cached_result) = self.cached_result.clone() {
            Ok(cached_result)
        } else {
            run_ix_with_free_tasks(
                self.task_key,
                &self.task,
                payer.pubkey(),
                next_available,
                lookup_tables,
            )
            .await
        };

        let run_ix = match maybe_run_ix {
            Ok(run_ix) => run_ix,
            Err(err) => return self.handle_ix_err(ctx.clone(), err).await,
        };

        tx_sender
            .send(TransactionTask {
                worth: self.task.crank_reward,
                task: TimedTask {
                    in_flight_task_ids: run_ix.free_task_ids,
                    ..self.clone()
                },
                instructions: run_ix.instructions,
                lookup_tables: Some(run_ix.lookup_tables),
            })
            .await?;

        Ok(())
    }

    pub async fn handle_completion(
        &self,
        ctx: Arc<TaskContext>,
        err: Option<TransactionQueueError>,
        tx_fee: u64,
    ) -> anyhow::Result<()> {
        ctx.task_state_client
            .remove_in_progress_tasks(
                self.task_queue_key,
                self.in_flight_task_ids.iter().cloned().collect(),
            )
            .await;
        let task_ids = ctx
            .task_state_client
            .get_in_progress_tasks(self.task_queue_key)
            .await
            .map_err(|_| anyhow::anyhow!("task state channel closed"))?;
        TASK_IDS_RESERVED
            .with_label_values(&[self.task_queue_name.as_str()])
            .set(task_ids.len() as i64);

        TASKS_IN_PROGRESS
            .with_label_values(&[self.task_queue_name.as_str()])
            .dec();

        // Record the result
        ctx.profitability
            .record_transaction_result(
                &self.task_queue_name,
                if err.is_none() {
                    self.task.crank_reward
                } else {
                    0
                },
                tx_fee,
            )
            .await;

        if let Some(err) = err {
            let label = match err {
                TransactionQueueError::SimulatedTransactionError(_) => "Simulated",
                TransactionQueueError::TransactionError(_) => "Transaction",
                TransactionQueueError::RawTransactionError(_) => "RawTransaction",
                TransactionQueueError::FeeTooHigh => "FeeTooHigh",
                TransactionQueueError::IxGroupTooLarge => "IxGroupTooLarge",
                TransactionQueueError::RawSimulatedTransactionError(_) => "RawSimulated",
                TransactionQueueError::RpcError(_) => "RpcError",
                TransactionQueueError::InstructionError(_) => "InstructionError",
                TransactionQueueError::SerializationError(_) => "SerializationError",
                TransactionQueueError::CompileError(_) => "CompileError",
                TransactionQueueError::SignerError(_) => "SignerError",
                TransactionQueueError::StaleTransaction => "StaleTransaction",
                TransactionQueueError::ChannelClosed => "ChannelClosed",
                TransactionQueueError::MaxRetriesExceeded => "MaxRetriesExceeded",
            };
            TASKS_FAILED
                .with_label_values(&[self.task_queue_name.as_str(), label])
                .inc();
            match err {
                TransactionQueueError::FeeTooHigh => {
                    info!(?self.task_key, ?err, "task fee too high");
                    ctx.task_queue
                        .add_task(TimedTask {
                            task: self.task.clone(),
                            total_retries: self.total_retries,
                            in_flight_task_ids: vec![],
                            profitability_delayed: self.profitability_delayed,
                            // Try again in 10-30 seconds
                            task_time: self.task_time + rand::random_range(10..30),
                            ..self.clone()
                        })
                        .await?;
                }
                // Handle task not found (simulated)
                TransactionQueueError::SimulatedTransactionError(
                    TransactionError::InstructionError(_, InstructionError::Custom(code)),
                ) if code == 3012 && ctx.rpc_client.get_account(&self.task_key).await.is_err() => {
                    info!(?self.task_key, "task not found, removing from queue");
                }
                // Handle task not found (real)
                TransactionQueueError::TransactionError(TransactionError::InstructionError(
                    _,
                    InstructionError::Custom(code),
                )) if code == 3012 && ctx.rpc_client.get_account(&self.task_key).await.is_err() => {
                    info!(?self.task_key, "task not found, removing from queue");
                }
                TransactionQueueError::RawTransactionError(_)
                | TransactionQueueError::SimulatedTransactionError(_)
                | TransactionQueueError::TransactionError(_)
                | TransactionQueueError::RpcError(_)
                | TransactionQueueError::ChannelClosed
                | TransactionQueueError::MaxRetriesExceeded => {
                    if self.total_retries < self.max_retries && !self.is_cleanup_task {
                        let base_delay = 30 * (1 << self.total_retries);
                        let jitter = rand::random_range(0..60); // Jitter up to 1 minute to prevent conflicts with other turners
                        let retry_delay = base_delay + jitter;
                        info!(
                            ?self.task_key,
                            ?self.task_time,
                            ?err,
                            ?retry_delay,
                            "task transaction failed, retrying"
                        );
                        let now = *ctx.now_rx.borrow();

                        ctx.task_queue
                            .add_task(TimedTask {
                                task: self.task.clone(),
                                total_retries: self.total_retries + 1,
                                // Try again when task is stale
                                task_time: now + retry_delay,
                                ..self.clone()
                            })
                            .await?;
                    } else if !self.is_cleanup_task {
                        info!(
                            "task {:?} failed after {} retries",
                            self.task_key, self.max_retries
                        );
                        let task_queue = self.get_task_queue(ctx.clone()).await?;
                        ctx.task_queue
                            .add_task(TimedTask {
                                task: self.task.clone(),
                                total_retries: 0,
                                task_time: self.task_time + task_queue.stale_task_age as u64,
                                in_flight_task_ids: vec![],
                                is_cleanup_task: true,
                                cached_result: None,
                                ..self.clone()
                            })
                            .await?;
                        TASKS_FAILED
                            .with_label_values(&[self.task_queue_name.as_str(), "RetriesExceeded"])
                            .inc();
                    }
                }
                TransactionQueueError::StaleTransaction => {
                    info!(?self.task_key, ?err, "task stale, trying again");
                    let now = *ctx.now_rx.borrow();
                    ctx.task_queue
                        .add_task(TimedTask {
                            task: self.task.clone(),
                            total_retries: self.total_retries,
                            in_flight_task_ids: vec![],
                            profitability_delayed: self.profitability_delayed,
                            // Try again immediately
                            task_time: now,
                            ..self.clone()
                        })
                        .await?;
                }
                _ => {
                    info!(?self.task_key, ?err, "task failed");
                }
            }
        } else {
            TASKS_COMPLETED
                .with_label_values(&[self.task_queue_name.as_str()])
                .inc();
        }
        Ok(())
    }
}

pub async fn process_tasks<T: Stream<Item = TimedTask> + Sized>(
    tasks: Box<T>,
    ctx: Arc<TaskContext>,
    handle: SubsystemHandle,
) -> anyhow::Result<()> {
    let fut = tasks
        .map(anyhow::Ok)
        .try_for_each_concurrent(Some(5), |task| {
            let ctx = ctx.clone();
            async move { task.clone().process(ctx).await }
        });
    tokio::select! {
        _ = handle.on_shutdown_requested() => {
            info!("shutdown requested, stopping tasks queue");
            Ok(())
        }
        res = fut => {
            info!("tasks queue finished");
            res
        }
    }
}
