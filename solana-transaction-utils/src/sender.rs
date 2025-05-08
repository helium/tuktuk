use crate::{
    blockhash_watcher,
    error::Error,
    queue::{CompletedTransactionTask, TransactionTask},
    tpu_conduit,
};
use dashmap::DashMap;
use futures::{stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::VersionedTransaction,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_graceful_shutdown::{SubsystemBuilder, SubsystemHandle};

const CONFIRMATION_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS: usize = 100;

#[derive(Clone, Debug)]
pub struct PackedTransactionWithTasks<T: Send + Clone> {
    pub instructions: Vec<Instruction>,
    pub tasks: Vec<TransactionTask<T>>,
    pub fee: u64,
    pub re_sign_count: u32,
}

impl<T: Send + Clone> PackedTransactionWithTasks<T> {
    pub fn with_incremented_re_sign_count(&self) -> Self {
        let mut result = self.clone();
        result.re_sign_count += 1;
        result
    }

    pub fn lookup_tables(&self) -> Vec<AddressLookupTableAccount> {
        self.tasks
            .iter()
            .flat_map(|t| t.lookup_tables.clone())
            .flatten()
            .collect_vec()
    }
}

#[derive(Debug, Clone)]
struct TransactionData<T: Send + Clone> {
    packed_tx: PackedTransactionWithTasks<T>,
    last_valid_block_height: u64,
    serialized_tx: Vec<u8>,
    sent_to_rpc: bool,
}

pub struct TransactionSender<T: Send + Clone + Sync> {
    unconfirmed_txs: Arc<DashMap<Signature, TransactionData<T>>>,
    rpc_client: Arc<RpcClient>,
    tpu_conduit: tpu_conduit::TpuConduit,
    result_tx: Sender<CompletedTransactionTask<T>>,
    payer: Arc<Keypair>,
    max_re_sign_count: u32,
}

impl<T: Send + Clone + Sync> TransactionSender<T> {
    pub async fn new(
        rpc_client: Arc<RpcClient>,
        ws_url: String,
        payer: Arc<Keypair>,
        result_tx: Sender<CompletedTransactionTask<T>>,
        max_re_sign_count: u32,
    ) -> Result<Self, Error> {
        let tpu_conduit = tpu_conduit::TpuConduit::new(rpc_client.clone(), ws_url);
        Ok(Self {
            unconfirmed_txs: Arc::new(DashMap::new()),
            rpc_client,
            tpu_conduit,
            result_tx,
            payer,
            max_re_sign_count,
        })
    }

    pub async fn handle_packed_tx(
        &self,
        packed_tx: PackedTransactionWithTasks<T>,
        blockhash_rx: &blockhash_watcher::MessageReceiver,
    ) -> Result<(), Error> {
        if let Err(e) = self.process_packed_tx(&packed_tx, blockhash_rx).await {
            // Handle processing error by notifying all tasks
            stream::iter(packed_tx.tasks)
                .map(|task| Ok((task, Some(e.clone()))))
                .try_for_each(|(task, err)| async move {
                    self.result_tx
                        .send(CompletedTransactionTask { err, task, fee: 0 })
                        .await
                })
                .await?
        }

        Ok(())
    }

    pub async fn process_packed_tx(
        &self,
        packed: &PackedTransactionWithTasks<T>,
        blockhash_rx: &blockhash_watcher::MessageReceiver,
    ) -> Result<(), Error> {
        // Check if transaction has been resigned too many times
        if packed.re_sign_count >= self.max_re_sign_count {
            return Err(Error::StaleTransaction);
        }

        let blockhash = blockhash_rx.borrow().last_valid_blockhash;
        let message = v0::Message::try_compile(
            &self.payer.pubkey(),
            &packed.instructions,
            &packed.lookup_tables(),
            blockhash,
        )?;

        let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&*self.payer])
            .map_err(Error::signer)?;

        let serialized = bincode::serialize(&tx).map_err(Error::serialization)?;
        let signature = tx.signatures[0];

        // Initial send via RPC
        self.send_transaction_with_rpc(&tx).await?;

        // Add to unconfirmed map
        self.unconfirmed_txs.insert(
            signature,
            TransactionData {
                packed_tx: packed.clone(),
                last_valid_block_height: blockhash_rx.borrow().last_valid_block_height,
                serialized_tx: serialized,
                sent_to_rpc: false,
            },
        );

        Ok(())
    }

    async fn re_handle_unconfirmed<'a, I: Iterator<Item = &'a Signature>>(
        &self,
        signatures: I,
        blockhash_rx: &blockhash_watcher::MessageReceiver,
    ) -> Result<(), Error> {
        for signature in signatures {
            // Blockhash expired, increment resign count and retry via rpc
            if let Some((_, data)) = self.unconfirmed_txs.remove(signature) {
                // Create new packed transaction with incremented resign count
                let new_packed = data.packed_tx.with_incremented_re_sign_count();
                self.handle_packed_tx(new_packed, blockhash_rx).await?;
            }
        }
        Ok(())
    }

    async fn send_transaction_with_rpc(&self, tx: &VersionedTransaction) -> Result<(), Error> {
        match self.rpc_client.send_transaction(tx).await {
            Ok(_) => {
                let sig = tx.signatures[0];
                if let Some(mut data) = self.unconfirmed_txs.get_mut(&sig) {
                    data.sent_to_rpc = true;
                }
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn check_and_retry(
        &mut self,
        blockhash_rx: &blockhash_watcher::MessageReceiver,
    ) -> Result<(), Error> {
        let current_height = blockhash_rx.borrow().current_block_height;
        // Check confirmations
        let signatures: Vec<_> = self.unconfirmed_txs.iter().map(|r| *r.key()).collect();
        if signatures.is_empty() {
            return Ok(());
        }

        for chunk in signatures.chunks(MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS) {
            if let Ok(result) = self.rpc_client.get_signature_statuses(chunk).await {
                let statuses = result.value;
                for (signature, status) in chunk.iter().zip(statuses.into_iter()) {
                    if let Some(status) = status {
                        if status.satisfies_commitment(self.rpc_client.commitment()) {
                            if let Some((_, data)) = self.unconfirmed_txs.remove(signature) {
                                // Send results for all tasks in this transaction
                                let num_packed_tasks = data.packed_tx.tasks.len();
                                for task in data.packed_tx.tasks {
                                    let err = status.err.clone().map(Error::TransactionError);
                                    self.result_tx
                                        .send(CompletedTransactionTask {
                                            err,
                                            task,
                                            fee: data
                                                .packed_tx
                                                .fee
                                                .div_ceil(num_packed_tasks as u64),
                                        })
                                        .await?
                                }
                            }
                        }
                    }
                }
            }
        }

        // Retry unconfirmed via TPU or resign if blockhash expired
        if !self.unconfirmed_txs.is_empty() && self.tpu_conduit.connect().await.is_ok() {
            let (unexpired, expired): (Vec<_>, Vec<_>) = self
                .unconfirmed_txs
                .iter()
                .partition(|entry| entry.value().last_valid_block_height < current_height);

            let unexpired_wire_txns = unexpired
                .iter()
                .map(|entry| entry.value().serialized_tx.clone())
                .collect_vec();
            if self
                .tpu_conduit
                .send_batch(unexpired_wire_txns)
                .await
                .is_err()
            {
                self.re_handle_unconfirmed(unexpired.iter().map(|entry| entry.key()), blockhash_rx)
                    .await?;
            }
            self.re_handle_unconfirmed(expired.iter().map(|entry| entry.key()), blockhash_rx)
                .await?;
        }

        Ok(())
    }

    pub async fn run(
        mut self,
        mut rx: Receiver<PackedTransactionWithTasks<T>>,
        handle: SubsystemHandle,
    ) -> Result<(), Error> {
        let mut blockhash_watcher = blockhash_watcher::BlockhashWatcher::new(
            blockhash_watcher::BLOCKHASH_REFRESH_INTERVAL,
            self.rpc_client.clone(),
        );
        handle.start(SubsystemBuilder::new("blockhash-updater", {
            let watcher = blockhash_watcher.clone();
            move |handle| watcher.run(handle)
        }));

        let mut check_interval = tokio::time::interval(CONFIRMATION_CHECK_INTERVAL);
        let blockchain_rx = blockhash_watcher.watcher();

        loop {
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    self.tpu_conduit.disconnect().await;
                    return Ok(());
                }
                Some(packed_tx) = rx.recv() => {
                    self.handle_packed_tx(packed_tx, &blockchain_rx).await?;
                }
                _ = check_interval.tick() => {
                    self.tpu_conduit.maybe_disconnect().await;
                    self.check_and_retry(&blockchain_rx).await?;
                }
            }
        }
    }
}
