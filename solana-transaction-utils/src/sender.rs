use crate::{
    blockhash_watcher,
    error::Error,
    queue::{CompletedTransactionTask, TransactionTask},
    send_and_confirm_transactions_in_parallel::QuicTpuClient,
};
use dashmap::DashMap;
use futures::{stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use solana_client::{
    nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
    tpu_client::TpuClientConfig,
};
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
use tracing::{error, info};

const SEND_TIMEOUT: Duration = Duration::from_secs(5);
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
    tpu_client: Option<QuicTpuClient>,
    ws_url: String,
    result_tx: Sender<CompletedTransactionTask<T>>,
    payer: Arc<Keypair>,
    max_re_sign_count: u32,
    last_tpu_use: u64,
}

impl<T: Send + Clone + Sync> TransactionSender<T> {
    pub async fn new(
        rpc_client: Arc<RpcClient>,
        ws_url: String,
        payer: Arc<Keypair>,
        result_tx: Sender<CompletedTransactionTask<T>>,
        max_re_sign_count: u32,
    ) -> Result<Self, Error> {
        Ok(Self {
            unconfirmed_txs: Arc::new(DashMap::new()),
            rpc_client,
            tpu_client: None,
            ws_url,
            result_tx,
            payer,
            max_re_sign_count,
            last_tpu_use: 0,
        })
    }

    async fn ensure_tpu_client(&mut self) -> Result<(), Error> {
        if self.tpu_client.is_none() {
            info!("Initializing TPU client");
            match TpuClient::new(
                "transaction-sender",
                self.rpc_client.clone(),
                &self.ws_url,
                TpuClientConfig::default(),
            )
            .await
            {
                Ok(new_client) => {
                    self.tpu_client = Some(new_client);
                }
                Err(e) => {
                    error!("Failed to initialize TPU client: {:?}", e);
                    return Err(Error::TpuSenderError(e.to_string()));
                }
            }
        }
        Ok(())
    }

    async fn shutdown_tpu_client(&mut self) {
        if let Some(tpu) = &mut self.tpu_client {
            info!("Shutting down TPU client due to inactivity");
            tpu.shutdown().await;
            self.tpu_client = None;
        }
    }

    const TPU_SHUTDOWN_THRESHOLD: u64 = 60; // Shutdown TPU client after 60 seconds of inactivity

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
        let current_time = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        {
            Ok(duration) => duration.as_secs(),
            Err(e) => {
                return Err(Error::SystemTimeError(e.to_string()));
            }
        };

        // Check if we should shutdown TPU client due to inactivity
        if self.last_tpu_use > 0
            && current_time.saturating_sub(self.last_tpu_use) > Self::TPU_SHUTDOWN_THRESHOLD
        {
            self.shutdown_tpu_client().await;
        }

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
        if !self.unconfirmed_txs.is_empty() && self.ensure_tpu_client().await.is_ok() {
            for entry in self.unconfirmed_txs.iter() {
                let signature = *entry.key();
                let data = entry.value();

                if current_height < data.last_valid_block_height {
                    if let Some(tpu) = &self.tpu_client {
                        match tokio::time::timeout(
                            SEND_TIMEOUT,
                            tpu.send_wire_transaction(data.serialized_tx.clone()),
                        )
                        .await
                        {
                            Ok(true) => {
                                self.last_tpu_use = current_time;
                            }
                            _ => {
                                // Blockhash expired, increment resign count and retry via rpc
                                self.unconfirmed_txs.remove(&signature);
                                // Create new packed transaction with incremented resign count
                                let new_packed = data.packed_tx.with_incremented_re_sign_count();
                                self.handle_packed_tx(new_packed, blockhash_rx).await?;
                            }
                        }
                    }
                } else {
                    // Blockhash expired, increment resign count and retry via rpc
                    self.unconfirmed_txs.remove(&signature);
                    // Create new packed transaction with incremented resign count
                    let new_packed = data.packed_tx.with_incremented_re_sign_count();
                    self.handle_packed_tx(new_packed, blockhash_rx).await?;
                }
            }
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
                    if let Some(tpu) = &mut self.tpu_client {
                        tpu.shutdown().await;
                    }
                    return Ok(());
                }
                Some(packed_tx) = rx.recv() => {
                    self.handle_packed_tx(packed_tx, &blockchain_rx).await?;
                }
                _ = check_interval.tick() => {
                    self.check_and_retry(&blockchain_rx).await?;
                }
            }
        }
    }
}
