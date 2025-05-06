use solana_transaction_utils::queue::{create_transaction_queue, TransactionQueueArgs};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::task_queue::TimedTask;

pub struct TransactionSenderSubsystem {
    args: TransactionQueueArgs<TimedTask>,
}

impl TransactionSenderSubsystem {
    pub fn new(args: TransactionQueueArgs<TimedTask>) -> Self {
        Self { args }
    }
    pub async fn run(self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        info!("starting transaction queue");
        let thread = create_transaction_queue::<TimedTask>(self.args);
        let result = tokio::select! {
        _ = subsys.on_shutdown_requested() => {
                    Ok(())
                },
                res = thread => {
                    res.map_err(|e| anyhow::anyhow!("transaction queue error: {}", e))
                }
            };
        info!("shutting down transaction queue");
        result
    }
}
