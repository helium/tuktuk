use std::collections::HashMap;

use solana_sdk::{
    instruction::Instruction, signature::Signature, transaction::VersionedTransaction,
};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{queue::TransactionTask, sync};

pub type TxTrackerSender<T> = sync::MessageSender<TxTrackerRequest<T>>;
pub type TxTrackerReceiver<T> = sync::MessageReceiver<TxTrackerRequest<T>>;

#[derive(Debug, Clone)]
pub struct TransactionData<T: Send + Clone + std::fmt::Debug> {
    pub packed_tx: PackedTransactionWithTasks<T>,
    pub tx: VersionedTransaction,
    pub last_valid_block_height: u64,
}

#[derive(Clone, Debug)]
pub struct PackedTransactionWithTasks<T: Send + Clone + std::fmt::Debug> {
    pub instructions: Vec<Instruction>,
    pub tasks: Vec<TransactionTask<T>>,
    pub fee: u64,
    pub re_sign_count: u32,
}

pub enum TxTrackerRequest<T: Send + Clone + std::fmt::Debug> {
    GetAll {
        resp: sync::ResponseSender<Box<HashMap<Signature, TransactionData<T>>>>,
    },
    Remove {
        signature: Signature,
        resp: sync::ResponseSender<Option<Box<TransactionData<T>>>>,
    },
    Add {
        signature: Signature,
        transaction_data: Box<TransactionData<T>>,
    },
}

impl<T: Send + Clone + std::fmt::Debug> TxTrackerSender<T> {
    pub async fn get_all(
        &self,
    ) -> Result<Box<HashMap<Signature, TransactionData<T>>>, crate::error::Error> {
        self.request(|resp| TxTrackerRequest::GetAll { resp }).await
    }

    pub async fn remove(
        &self,
        signature: Signature,
    ) -> Result<Option<Box<TransactionData<T>>>, crate::error::Error> {
        self.request(|resp| TxTrackerRequest::Remove { signature, resp })
            .await
    }

    pub async fn add(&self, signature: Signature, transaction_data: Box<TransactionData<T>>) {
        self.send(TxTrackerRequest::Add {
            signature,
            transaction_data,
        })
        .await
    }
}

pub fn tx_tracker_channel<T: Send + Clone + std::fmt::Debug>(
) -> (TxTrackerSender<T>, TxTrackerReceiver<T>) {
    let (tx, rx) = sync::message_channel(100);
    (tx, rx)
}

pub struct TxTracker<T: Send + Clone + std::fmt::Debug> {
    cache: HashMap<Signature, TransactionData<T>>,
    receiver: TxTrackerReceiver<T>,
}

impl<T: Send + Clone + std::fmt::Debug> TxTracker<T> {
    pub fn new(receiver: TxTrackerReceiver<T>) -> Self {
        Self {
            cache: HashMap::new(),
            receiver,
        }
    }

    pub async fn run(mut self, handle: SubsystemHandle) -> Result<(), crate::error::Error> {
        info!("starting tx tracker cache");
        loop {
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    info!("shutting down task queue cache");
                    break;
                }
                Some(req) = self.receiver.recv() => {
                    match req {
                        TxTrackerRequest::GetAll { resp } => {
                            resp.send(Box::new(self.cache.clone()));
                        }
                        TxTrackerRequest::Remove { signature, resp } => {
                            resp.send(self.cache.remove(&signature).map(Box::new));
                        }
                        TxTrackerRequest::Add { signature, transaction_data } => {
                            self.cache.insert(signature, *transaction_data);
                        }
                    }
                }
                else => break,
            }
        }
        Ok(())
    }
}
