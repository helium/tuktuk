use std::sync::Arc;

use solana_client::nonblocking::rpc_client::RpcClient;
use tokio::sync::watch::Receiver;
use tuktuk_sdk::watcher::PubsubTracker;

use crate::{settings::TaskQueueFilter, task_queue::TaskQueue};

#[derive(Clone)]
pub struct WatcherArgs {
    pub max_retries: u8,
    pub min_crank_fee: u64,
    pub rpc_client: Arc<RpcClient>,
    pub pubsub_tracker: Arc<PubsubTracker>,
    pub now: Receiver<u64>,
    pub task_queue: Arc<TaskQueue>,
    pub task_queue_filter: Arc<TaskQueueFilter>,
}
