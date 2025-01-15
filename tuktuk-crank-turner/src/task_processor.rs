use std::{collections::HashSet, sync::Arc};

use futures::{Stream, StreamExt, TryStreamExt};
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{commitment_config::CommitmentLevel, signer::Signer, transaction::Transaction};
use solana_transaction_utils::queue::{TransactionQueueError, TransactionTask};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;
use tuktuk_sdk::compiled_transaction::run_ix;

use crate::{task_context::TaskContext, task_queue::TimedTask};

impl TimedTask {
    pub async fn process(&self, ctx: Arc<TaskContext>) -> anyhow::Result<()> {
        let TaskContext {
            rpc_client,
            payer,
            tx_sender,
            in_progress_tasks,
            ..
        } = &*ctx;

        let mut in_progress = in_progress_tasks.lock().await;
        let task_ids = in_progress
            .entry(self.task_queue_key)
            .or_insert_with(HashSet::new);

        let maybe_run_ix =
            run_ix(rpc_client.as_ref(), self.task_key, payer.pubkey(), task_ids).await;

        if let Err(err) = maybe_run_ix {
            info!(?self, ?err, "getting instructions failed");
            if self.total_retries < self.max_retries {
                ctx.task_queue
                    .add_task(TimedTask {
                        total_retries: self.total_retries + 1,
                        // Try again in 30 seconds
                        task_time: self.task_time + 30,
                        task_key: self.task_key,
                        task_queue_key: self.task_queue_key,
                        max_retries: self.max_retries,
                        in_flight_task_ids: self.in_flight_task_ids.clone(),
                    })
                    .await?;
            }
            return Ok(());
        }

        let run_ix = maybe_run_ix.unwrap();

        let ctx = ctx.clone();
        if let Some(run_ix) = run_ix {
            task_ids.extend(run_ix.free_task_ids.clone());
            let recent_blockhash = rpc_client.get_latest_blockhash().await?;
            let mut tx = Transaction::new_with_payer(&run_ix.instructions, Some(&payer.pubkey()));
            tx.message.recent_blockhash = recent_blockhash;
            let simulated = rpc_client
                .simulate_transaction_with_config(
                    &tx,
                    RpcSimulateTransactionConfig {
                        commitment: Some(
                            solana_sdk::commitment_config::CommitmentConfig::confirmed(),
                        ),
                        sig_verify: true,
                        ..Default::default()
                    },
                )
                .await;
            // info!(?simulated, "simulated");
            match simulated {
                Ok(simulated) => {
                    if let Some(err) = simulated.value.err {
                        info!(
                                    ?self,
                            ?err,
                            ?simulated.value.logs,
                            "task simulation failed",
                        );
                        drop(in_progress);
                        return self
                            .handle_completion(
                                ctx,
                                Some(TransactionQueueError::TransactionError(err)),
                            )
                            .await;
                    }
                }
                Err(err) => {
                    drop(in_progress);
                    info!(?self, ?err, "task simulation failed");
                    return self
                        .handle_completion(
                            ctx,
                            Some(TransactionQueueError::RawTransactionError(err.to_string())),
                        )
                        .await;
                }
            }

            tx_sender
                .send(TransactionTask {
                    task: TimedTask {
                        in_flight_task_ids: run_ix.free_task_ids,
                        ..self.clone()
                    },
                    instructions: run_ix.instructions,
                })
                .await?;
        }

        Ok(())
    }

    pub async fn handle_completion(
        &self,
        ctx: Arc<TaskContext>,
        err: Option<TransactionQueueError>,
    ) -> anyhow::Result<()> {
        let mut in_progress = ctx.in_progress_tasks.lock().await;
        let task_ids = in_progress
            .entry(self.task_queue_key)
            .or_insert_with(HashSet::new);
        for task_id in &self.in_flight_task_ids {
            task_ids.remove(task_id);
        }
        drop(in_progress);
        if let Some(err) = err {
            if matches!(err, TransactionQueueError::FeeTooHigh) {
                info!(?self, ?err, "task fee too high");
                ctx.task_queue
                    .add_task(TimedTask {
                        total_retries: 0,
                        // Try again in 10 seconds
                        task_time: self.task_time + 10,
                        task_key: self.task_key,
                        task_queue_key: self.task_queue_key,
                        max_retries: self.max_retries,
                        in_flight_task_ids: vec![],
                    })
                    .await?;
            } else {
                info!(?self, ?err, "task failed");
                if self.total_retries < self.max_retries {
                    ctx.task_queue
                        .add_task(TimedTask {
                            total_retries: self.total_retries + 1,
                            // Try again in 30 seconds
                            task_time: self.task_time + 30,
                            task_key: self.task_key,
                            task_queue_key: self.task_queue_key,
                            max_retries: self.max_retries,
                            in_flight_task_ids: self.in_flight_task_ids.clone(),
                        })
                        .await?;
                } else {
                    info!(
                        "task {:?} failed after {} retries",
                        self.task_key, self.max_retries
                    );
                }
            }
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
            async move { task.process(ctx).await }
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
