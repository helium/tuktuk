use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use futures::{stream, Stream, StreamExt, TryFutureExt, TryStreamExt};
use itertools::Itertools;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::VersionedTransaction,
};
use solana_transaction_status::TransactionStatus;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_graceful_shutdown::{SubsystemBuilder, SubsystemHandle};
use tracing::warn;

use crate::{
    blockhash_watcher,
    error::Error,
    queue::{CompletedTransactionTask, TransactionTask},
};

const CONFIRMATION_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS: usize = 100;
const RPC_TXN_SEND_CONCURRENCY: usize = 50;

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

    pub fn into_completions_with_status<E: Into<Error>>(
        self,
        err: Option<E>,
        fee: Option<u64>,
    ) -> impl Stream<Item = CompletedTransactionTask<T>> {
        let err = err.map(Into::into);
        let num_tasks = self.tasks.len();
        stream::iter(self.tasks).map(move |task| CompletedTransactionTask {
            err: err.clone(),
            fee: fee.unwrap_or_else(|| self.fee.div_ceil(num_tasks as u64)),
            task,
        })
    }

    pub fn mk_transaction<S: Signer>(
        &self,
        max_re_sign_count: u32,
        blockhash: Hash,
        signer: S,
    ) -> Result<VersionedTransaction, Error> {
        if self.re_sign_count >= max_re_sign_count {
            return Err(Error::StaleTransaction);
        }

        let message = v0::Message::try_compile(
            &signer.pubkey(),
            &self.instructions,
            &self.lookup_tables(),
            blockhash,
        )?;

        VersionedTransaction::try_new(VersionedMessage::V0(message), &[signer])
            .map_err(Error::signer)
    }
}

#[derive(Debug, Clone)]
struct TransactionData<T: Send + Clone> {
    packed_tx: PackedTransactionWithTasks<T>,
    tx: VersionedTransaction,
    last_valid_block_height: u64,
}

pub struct TransactionSender<T: Send + Clone + Sync> {
    unconfirmed_txs: Arc<DashMap<Signature, TransactionData<T>>>,
    rpc_client: Arc<RpcClient>,
    result_tx: Sender<CompletedTransactionTask<T>>,
    payer: Arc<Keypair>,
    max_re_sign_count: u32,
}

impl<T: Send + Clone + Sync> TransactionSender<T> {
    pub async fn new(
        rpc_client: Arc<RpcClient>,
        payer: Arc<Keypair>,
        result_tx: Sender<CompletedTransactionTask<T>>,
        max_re_sign_count: u32,
    ) -> Result<Self, Error> {
        Ok(Self {
            unconfirmed_txs: Arc::new(DashMap::new()),
            rpc_client,
            result_tx,
            payer,
            max_re_sign_count,
        })
    }

    async fn handle_packed_tx(
        &self,
        packed_tx: PackedTransactionWithTasks<T>,
        blockhash_rx: &blockhash_watcher::MessageReceiver,
    ) {
        let blockhash = blockhash_rx.borrow().last_valid_blockhash;
        // Convert the packed txn into a versioned transaction. If this fails it's not recoverable through retries
        // so notify and exit on errors without queueing for retries
        let tx = match packed_tx.mk_transaction(self.max_re_sign_count, blockhash, &self.payer) {
            Ok(tx) => tx,
            Err(err) => {
                self.handle_completions(packed_tx.into_completions_with_status(Some(err), Some(0)))
                    .await;
                return;
            }
        };
        // Send transaction. Queue for checks and retries whether errored or not to handle rpc being down
        // temporarily
        let _ = self
            .rpc_client
            .send_transaction(&tx)
            .map_err(Error::from)
            .inspect_err(|err| warn!(?err, "sending transaction"))
            .await;

        self.unconfirmed_txs.insert(
            tx.signatures[0],
            TransactionData {
                tx,
                packed_tx: packed_tx.clone(),
                last_valid_block_height: blockhash_rx.borrow().last_valid_block_height,
            },
        );
    }

    pub fn send_transactions<'a>(
        &'a self,
        txns: &'a [VersionedTransaction],
    ) -> impl Stream<Item = (Signature, Result<(), Error>)> + use<'a, T> {
        stream::iter(txns)
            .map(move |txn| {
                let signature = txn.signatures[0];
                let rpc_client = self.rpc_client.clone();
                async move {
                    (
                        signature,
                        rpc_client
                            .send_transaction(txn)
                            .map_ok(|_| ())
                            .map_err(Error::from)
                            .await,
                    )
                }
            })
            .buffer_unordered(RPC_TXN_SEND_CONCURRENCY)
    }

    async fn handle_expired<I: Stream<Item = Signature>>(
        &self,
        signatures: I,
        blockhash_rx: &blockhash_watcher::MessageReceiver,
    ) {
        signatures
            .filter_map(|signature| async move {
                self.unconfirmed_txs
                    .remove(&signature)
                    .map(|(_, data)| data.packed_tx.with_incremented_re_sign_count())
            })
            .for_each_concurrent(RPC_TXN_SEND_CONCURRENCY, |packed_tx| async move {
                self.handle_packed_tx(packed_tx, blockhash_rx).await
            })
            .await
    }

    async fn handle_completed<I: Stream<Item = (Signature, TransactionStatus)>>(
        &self,
        signature_statuses: I,
    ) {
        let completions = signature_statuses
            // Look up transaction data for signature
            .filter_map(|(signature, status)| async move {
                self.unconfirmed_txs
                    .remove(&signature)
                    .map(|(_, v)| (v, status))
            })
            // Map status into completion messages
            .flat_map(|(data, status)| {
                data.packed_tx
                    .into_completions_with_status(status.err.map(Error::TransactionError), None)
            });
        self.handle_completions(completions).await
    }

    async fn handle_completions<S: Stream<Item = CompletedTransactionTask<T>>>(
        &self,
        completions: S,
    ) {
        let _ = completions
            .map(Ok)
            .try_for_each(|completion| async move { self.result_tx.send(completion).await })
            .map_err(Error::from)
            .inspect_err(|err| warn!(?err, "sending task completions"))
            .await;
    }

    async fn handle_tick(&mut self, blockhash_rx: &blockhash_watcher::MessageReceiver) {
        // Check confirmations and process as completed
        let signatures = self.unconfirmed_txs.iter().map(|r| *r.key()).collect_vec();
        // Make a stream of completed (signature, status) tuples
        let completed_txns = stream::iter(signatures)
            .chunks(MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS)
            .then(|signatures| {
                let rpc_client = self.rpc_client.clone();
                let commitment = rpc_client.commitment();
                async move {
                    let signature_statuses = rpc_client
                        .get_signature_statuses(&signatures)
                        .map_ok_or_else(
                            |_| {
                                std::iter::repeat_n(None, signatures.len())
                                    .collect_vec()
                                    .into_iter()
                            },
                            |response| response.value.into_iter(),
                        )
                        .await
                        .zip(signatures)
                        .filter_map(move |(maybe_status, signature)| {
                            maybe_status.and_then(|status| {
                                status
                                    .satisfies_commitment(commitment)
                                    .then_some((signature, status))
                            })
                        });
                    stream::iter(signature_statuses)
                }
            })
            .flatten_unordered(10);
        // Remove completed and notify
        self.handle_completed(completed_txns).await;

        // Retry unconfirmed
        let current_height = blockhash_rx.borrow().current_block_height;
        if !self.unconfirmed_txs.is_empty() {
            let (unexpired, expired): (Vec<_>, Vec<_>) = self
                .unconfirmed_txs
                .iter()
                .partition(|entry| entry.value().last_valid_block_height < current_height);

            // Resend unexpred/unconfirmed to rpc
            let unexpired_txns = unexpired
                .iter()
                .map(|entry| entry.value().tx.clone())
                .collect_vec();
            // Collect failed transactions (likely expired) and handle as expired
            let unexpired_error_signatures = self
                .send_transactions(unexpired_txns.as_slice())
                .filter_map(|(signature, result)| async move { result.err().map(|_| signature) });
            let expired_signatures = expired.iter().map(|entry| entry.key().to_owned());

            self.handle_expired(unexpired_error_signatures, blockhash_rx)
                .await;
            self.handle_expired(stream::iter(expired_signatures), blockhash_rx)
                .await;
        }
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
                    return Ok(());
                }
                Some(packed_tx) = rx.recv() => {
                    self.handle_packed_tx(packed_tx, &blockchain_rx).await;
                }
                _ = check_interval.tick() => {
                    self.handle_tick(&blockchain_rx).await;
                }
            }
        }
    }
}
