use std::sync::Arc;

use futures::{Stream, StreamExt, TryStreamExt};
use solana_sdk::{
    signer::Signer,
    transaction::{Transaction, TransactionError},
};
use solana_transaction_utils::queue::TransactionTask;
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
            ..
        } = &*ctx;
        let run_ix = run_ix(rpc_client.as_ref(), self.task_key, payer.pubkey()).await?;
        if let Some(run_ix) = run_ix {
            let recent_blockhash = rpc_client.get_latest_blockhash().await?;
            let mut tx = Transaction::new_with_payer(&[run_ix.clone()], Some(&payer.pubkey()));
            tx.message.recent_blockhash = recent_blockhash;
            let simulated = rpc_client.simulate_transaction(&tx).await?;
            if let Some(err) = simulated.value.err {
                info!(
                    ?self,
                    ?err,
                    ?simulated.value.logs,
                    "task simulation failed",
                );
                return self.handle_completion(ctx, Some(err)).await;
            }

            tx_sender
                .send(TransactionTask {
                    task: self.clone(),
                    instructions: vec![run_ix],
                })
                .await?;
        }

        Ok(())
    }

    pub async fn handle_completion(
        &self,
        ctx: Arc<TaskContext>,
        err: Option<TransactionError>,
    ) -> anyhow::Result<()> {
        if let Some(err) = err {
            info!(?self, ?err, "task failed");
            if self.total_retries < self.max_retries {
                ctx.task_queue
                    .add_task(TimedTask {
                        total_retries: self.total_retries + 1,
                        // Try again in 30 seconds
                        task_time: self.task_time + 30,
                        task_key: self.task_key,
                        max_retries: self.max_retries,
                    })
                    .await?;
            } else {
                info!(
                    "task {:?} failed after {} retries",
                    self.task_key, self.max_retries
                );
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
