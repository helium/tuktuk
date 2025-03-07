use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount, pubkey::Pubkey, signature::Keypair,
};
use solana_transaction_utils::queue::TransactionTask;
use tokio::sync::{mpsc::Sender, watch::Receiver, Mutex};
use tuktuk_program::TaskQueueV0;

use crate::{
    profitability::TaskQueueProfitability,
    task_queue::{TaskQueue, TimedTask},
};
// Disallow concurrent transactions on the same queue node since the IDs will conflict.
pub type QueueNodeSegmentsInProgress = Arc<Mutex<HashSet<u32>>>;

pub struct TaskContext {
    pub tx_sender: Sender<TransactionTask<TimedTask>>,
    pub task_queue: Arc<TaskQueue>,
    pub now_rx: Receiver<u64>,
    pub rpc_client: Arc<RpcClient>,
    pub payer: Arc<Keypair>,
    pub in_progress_tasks: Arc<Mutex<HashMap<Pubkey, HashSet<u16>>>>,
    // Known lookup tables for the various task queues.
    pub lookup_tables: Arc<Mutex<HashMap<Pubkey, AddressLookupTableAccount>>>,
    pub task_queues: Arc<Mutex<HashMap<Pubkey, TaskQueueV0>>>,
    pub profitability: Arc<TaskQueueProfitability>,
}
