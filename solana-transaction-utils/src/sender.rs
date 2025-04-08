use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use dashmap::DashMap;
use itertools::Itertools;
use solana_client::{
    nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
    tpu_client::TpuClientConfig,
};
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::VersionedTransaction,
};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    RwLock,
};
use tokio_graceful_shutdown::{SubsystemBuilder, SubsystemHandle};
use tracing::{error, info};

use crate::{
    error::Error,
    queue::{CompletedTransactionTask, TransactionTask},
    send_and_confirm_transactions_in_parallel::QuicTpuClient,
};

const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIRMATION_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const BLOCKHASH_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS: usize = 100;

#[derive(Clone, Debug)]
pub struct PackedTransactionWithTasks<T: Send + Clone> {
    pub instructions: Vec<Instruction>,
    pub tasks: Vec<TransactionTask<T>>,
    pub fee: u64,
    pub re_sign_count: u32,
}

#[derive(Debug, Clone)]
struct TransactionData<T: Send + Clone> {
    packed_tx: PackedTransactionWithTasks<T>,
    last_valid_block_height: u64,
    serialized_tx: Vec<u8>,
    sent_to_rpc: bool,
}

#[derive(Clone, Debug, Copy)]
pub struct BlockHashData {
    last_valid_block_height: u64,
    blockhash: Hash,
}

pub struct TransactionSender<T: Send + Clone + Sync> {
    unconfirmed_txs: Arc<DashMap<Signature, TransactionData<T>>>,
    blockhash_data: Arc<RwLock<BlockHashData>>,
    current_block_height: Arc<AtomicU64>,
    rpc_client: Arc<RpcClient>,
    tpu_client: Option<QuicTpuClient>,
    ws_url: String,
    result_tx: Sender<CompletedTransactionTask<T>>,
    payer: Arc<Keypair>,
    max_re_sign_count: u32,
    last_tpu_use: u64,
}

pub async fn spawn_background_tasks(
    handle: SubsystemHandle,
    blockhash_data: Arc<RwLock<BlockHashData>>,
    current_block_height: Arc<AtomicU64>,
    rpc_client: Arc<RpcClient>,
) -> Result<(), Error> {
    // Spawn blockhash updater
    let mut interval = tokio::time::interval(BLOCKHASH_REFRESH_INTERVAL);
    loop {
        tokio::select! {
            _ = handle.on_shutdown_requested() => {
                return Ok(());
            }
            _ = interval.tick() => {
                if let Ok((blockhash, last_valid_block_height)) = rpc_client
                    .get_latest_blockhash_with_commitment(rpc_client.commitment())
                    .await
                {
                    *blockhash_data.write().await = BlockHashData {
                        last_valid_block_height,
                        blockhash,
                    };
                }
                if let Ok(block_height) = rpc_client.get_block_height().await {
                    current_block_height.store(block_height, Ordering::Relaxed);
                }
            }
        }
    }
}

impl<T: Send + Clone + Sync> TransactionSender<T> {
    pub async fn new(
        rpc_client: Arc<RpcClient>,
        ws_url: String,
        payer: Arc<Keypair>,
        result_tx: Sender<CompletedTransactionTask<T>>,
        max_re_sign_count: u32,
    ) -> Result<Self, Error> {
        // Initialize blockhash data
        let (blockhash, last_valid_block_height) = rpc_client
            .get_latest_blockhash_with_commitment(rpc_client.commitment())
            .await?;

        let blockhash_data = Arc::new(RwLock::new(BlockHashData {
            last_valid_block_height,
            blockhash,
        }));

        // Initialize block height
        let block_height = rpc_client.get_block_height().await?;
        let current_block_height = Arc::new(AtomicU64::new(block_height));

        Ok(Self {
            unconfirmed_txs: Arc::new(DashMap::new()),
            blockhash_data,
            current_block_height,
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

    pub async fn process_packed_tx(
        &self,
        packed: &PackedTransactionWithTasks<T>,
    ) -> Result<(), Error> {
        // Check if transaction has been resigned too many times
        if packed.re_sign_count >= self.max_re_sign_count {
            return Err(Error::StaleTransaction);
        }

        let blockhash = self.blockhash_data.read().await.blockhash;
        let lookup_tables = packed
            .tasks
            .iter()
            .flat_map(|t| t.lookup_tables.clone())
            .flatten()
            .collect_vec();
        let message = v0::Message::try_compile(
            &self.payer.pubkey(),
            &packed.instructions,
            &lookup_tables,
            blockhash,
        )?;

        let tx =
            VersionedTransaction::try_new(VersionedMessage::V0(message.clone()), &[&*self.payer])
                .map_err(|e| Error::SignerError(e.to_string()))?;

        let serialized =
            bincode::serialize(&tx).map_err(|e| Error::SerializationError(e.to_string()))?;
        let signature = tx.signatures[0];

        // Initial send via RPC
        self.send_transaction_with_rpc(&tx).await?;

        // Add to unconfirmed map
        self.unconfirmed_txs.insert(
            signature,
            TransactionData {
                packed_tx: packed.clone(),
                last_valid_block_height: self.blockhash_data.read().await.last_valid_block_height,
                serialized_tx: serialized.clone(),
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

    async fn check_and_retry(&mut self) {
        let current_height = self.current_block_height.load(Ordering::Relaxed);
        let current_time = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        {
            Ok(duration) => duration.as_secs(),
            Err(e) => {
                error!("System time error: {}", e);
                return;
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
            return;
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
                                        .await
                                        .expect("send result");
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
                                let mut new_packed = data.packed_tx.clone();
                                new_packed.re_sign_count += 1;

                                if let Err(e) = self.process_packed_tx(&new_packed).await {
                                    // Handle processing error by notifying all tasks
                                    for task in new_packed.tasks {
                                        self.result_tx
                                            .send(CompletedTransactionTask {
                                                err: Some(e.clone()),
                                                task,
                                                fee: 0,
                                            })
                                            .await
                                            .expect("send result");
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Blockhash expired, increment resign count and retry via rpc
                    self.unconfirmed_txs.remove(&signature);

                    // Create new packed transaction with incremented resign count
                    let mut new_packed = data.packed_tx.clone();
                    new_packed.re_sign_count += 1;

                    if let Err(e) = self.process_packed_tx(&new_packed).await {
                        // Handle processing error by notifying all tasks
                        for task in new_packed.tasks {
                            self.result_tx
                                .send(CompletedTransactionTask {
                                    err: Some(e.clone()),
                                    task,
                                    fee: 0,
                                })
                                .await
                                .expect("send result");
                        }
                    }
                }
            }
        }
    }

    pub async fn run(
        mut self,
        mut rx: Receiver<PackedTransactionWithTasks<T>>,
        handle: SubsystemHandle,
    ) -> Result<(), Error> {
        handle.start(SubsystemBuilder::new("blockhash-updater", {
            let blockhash_data = self.blockhash_data.clone();
            let current_block_height = self.current_block_height.clone();
            let rpc_client = self.rpc_client.clone();
            move |handle| {
                spawn_background_tasks(handle, blockhash_data, current_block_height, rpc_client)
            }
        }));

        let mut check_interval = tokio::time::interval(CONFIRMATION_CHECK_INTERVAL);

        loop {
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    if let Some(tpu) = &mut self.tpu_client {
                        tpu.shutdown().await;
                    }
                    return Ok(());
                }
                Some(packed_tx) = rx.recv() => {
                    if let Err(e) = self.process_packed_tx(&packed_tx).await {
                        // Handle processing error by notifying all tasks
                        for task in packed_tx.tasks {
                            self.result_tx
                                .send(CompletedTransactionTask {
                                    err: Some(e.clone()),
                                    task,
                                    fee: 0,
                                })
                                .await
                                .expect("send result");
                        }
                    }
                }
                _ = check_interval.tick() => {
                    self.check_and_retry().await;
                }
            }
        }
    }
}
