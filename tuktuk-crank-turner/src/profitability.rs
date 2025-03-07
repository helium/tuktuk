use std::collections::HashMap;

use tokio::sync::RwLock;

use crate::metrics::TASK_QUEUE_PROFIT;

#[derive(Default)]
pub struct TaskQueueProfitability {
    // Track running profit/loss per queue
    profits: RwLock<HashMap<String, i64>>,
}

impl TaskQueueProfitability {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record_transaction_result(&self, queue_name: &str, reward: u64, tx_fee: u64) {
        let mut profits = self.profits.write().await;
        let profit = profits.entry(queue_name.to_string()).or_insert(0);

        // If successful, add reward and subtract fees
        *profit += reward as i64 - tx_fee as i64;
        TASK_QUEUE_PROFIT
            .with_label_values(&[queue_name])
            .set(*profit as i64);
    }

    pub async fn should_delay(&self, queue_name: &str) -> bool {
        let profits = self.profits.read().await;
        let profit = profits.get(queue_name).unwrap_or(&0);

        *profit < 0
    }
}
