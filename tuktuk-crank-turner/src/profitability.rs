use std::collections::{HashMap, VecDeque};

use tokio::sync::RwLock;

use crate::metrics::{TASK_QUEUE_PROFIT, TASK_ROLLING_PROFIT_PER_TASK};

#[derive(Default)]
pub struct TaskQueueProfitability {
    recent_attempts_window: usize,
    // Track running profit/loss per queue
    profits: RwLock<HashMap<String, i64>>,
    // Track recent profits/losses per queue (last N attempts)
    recent_profits: RwLock<HashMap<String, VecDeque<i64>>>,
}

impl TaskQueueProfitability {
    pub fn new(recent_attempts_window: usize) -> Self {
        Self {
            recent_attempts_window,
            ..Default::default()
        }
    }

    pub async fn record_transaction_result(&self, queue_name: &str, reward: u64, tx_fee: u64) {
        // Don't bother tracking noops
        if reward == 0 && tx_fee == 0 {
            return;
        }
        let mut profits = self.profits.write().await;
        let mut recent_profits = self.recent_profits.write().await;

        let profit_amount = reward as i64 - tx_fee as i64;

        // Update all-time profits
        let profit = profits.entry(queue_name.to_string()).or_insert(0);
        *profit += profit_amount;

        // Update recent profits
        let recent = recent_profits
            .entry(queue_name.to_string())
            .or_insert_with(VecDeque::new);
        if recent.len() >= self.recent_attempts_window {
            recent.pop_front();
        }
        recent.push_back(profit_amount);

        // Update metric with all-time profit
        TASK_QUEUE_PROFIT
            .with_label_values(&[queue_name])
            .set(*profit);

        // Update metric with average of recent profits
        let recent_avg = if recent.is_empty() {
            profit_amount
        } else {
            recent.iter().sum::<i64>() / recent.len() as i64
        };
        TASK_ROLLING_PROFIT_PER_TASK
            .with_label_values(&[queue_name])
            .set(recent_avg);
    }

    pub async fn should_delay(&self, queue_name: &str) -> bool {
        let recent_profits = self.recent_profits.read().await;

        // Check recent profits
        if let Some(recent) = recent_profits.get(queue_name) {
            let recent_sum: i64 = recent.iter().sum();
            if recent_sum < 0 {
                return true;
            }
        }

        false
    }
}
