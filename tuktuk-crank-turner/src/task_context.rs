use std::{collections::HashSet, sync::Arc};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use solana_transaction_utils::queue::TransactionTask;
use tokio::sync::{mpsc::Sender, watch::Receiver, Mutex};

use crate::task_queue::{TaskQueue, TimedTask};

// Disallow concurrent transactions on the same queue node since the IDs will conflict.
pub type QueueNodeSegmentsInProgress = Arc<Mutex<HashSet<u32>>>;

pub struct TaskContext {
    pub tx_sender: Sender<TransactionTask<TimedTask>>,
    pub task_queue: Arc<TaskQueue>,
    pub now_rx: Receiver<u64>,
    pub rpc_client: Arc<RpcClient>,
    pub payer: Arc<Keypair>,
}
