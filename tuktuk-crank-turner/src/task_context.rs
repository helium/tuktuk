use std::sync::Arc;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use solana_transaction_utils::queue::TransactionTask;
use tokio::sync::{mpsc::Sender, watch};

use crate::{
    cache::{LookupTablesSender, TaskQueuesSender, TaskStateSender},
    profitability::TaskQueueProfitability,
    task_queue::{TaskQueue, TimedTask},
};

pub struct TaskContext {
    pub tx_sender: Sender<TransactionTask<TimedTask>>,
    pub task_queue: Arc<TaskQueue>,
    pub now_rx: watch::Receiver<u64>,
    pub rpc_client: Arc<RpcClient>,
    pub payer: Arc<Keypair>,
    pub task_state_client: TaskStateSender,
    pub lookup_tables_client: LookupTablesSender,
    pub task_queues_client: TaskQueuesSender,
    pub profitability: Arc<TaskQueueProfitability>,
}
