use std::sync::Arc;

use solana_transaction_utils::queue::CompletedTransactionTask;
use tokio::sync::mpsc::Receiver;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{task_context::TaskContext, task_queue::TimedTask};

pub async fn process_task_completions(
    completion_receiver: Receiver<CompletedTransactionTask<TimedTask>>,
    ctx: Arc<TaskContext>,
    handle: SubsystemHandle,
) -> anyhow::Result<()> {
    let mut completion_receiver = completion_receiver;
    loop {
        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                    info!("shutdown requested, stopping transaction queue");
                    break;
                }
                Some(result) = completion_receiver.recv() => {
            result
                .task
                .task
                .handle_completion(ctx.clone(), result.err)
                .await
                .expect("Failed to handle completion");
        }
         else => {
                    info!("all senders have been dropped, stopping transaction queue");
                    break;
                }
        }
    }
    info!("shutting down transaction completion queue");
    Ok(())
}
