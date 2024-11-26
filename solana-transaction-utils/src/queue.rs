use std::{sync::Arc, time::Duration};

use solana_client::{
    nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
    send_and_confirm_transactions_in_parallel::{
        send_and_confirm_transactions_in_parallel, SendAndConfirmConfig,
    },
    tpu_client::TpuClientConfig,
};
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, message::Message,
    signature::Keypair, signer::Signer, transaction::TransactionError,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinHandle,
    time::interval,
};

use crate::{
    pack::pack_instructions_into_transactions, priority_fee::auto_compute_limit_and_price,
};

#[derive(Debug, Clone)]
pub struct TransactionTask<T: Send + Clone> {
    pub task: T,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionQueueError {
    #[error("Transaction error: {0}")]
    TransactionError(#[from] TransactionError),
    #[error("Fee too high")]
    FeeTooHigh,
}

#[derive(Debug)]
pub struct CompletedTransactionTask<T: Send + Clone> {
    pub err: Option<TransactionQueueError>,
    pub task: TransactionTask<T>,
}

pub struct TransactionQueueArgs<T: Send + Clone> {
    pub rpc_client: Arc<RpcClient>,
    pub ws_url: String,
    pub payer: Arc<Keypair>,
    pub batch_duration: Duration,
    pub receiver: Receiver<TransactionTask<T>>,
    pub result_sender: Sender<CompletedTransactionTask<T>>,
    pub max_sol_fee: u64,
}

pub struct TransactionQueueHandles<T: Send + Clone> {
    pub sender: Sender<TransactionTask<T>>,
    pub receiver: Receiver<TransactionTask<T>>,
    pub result_sender: Sender<CompletedTransactionTask<T>>,
    pub result_receiver: Receiver<CompletedTransactionTask<T>>,
}

pub fn create_transaction_queue_handles<T: Send + Clone>(
    channel_capacity: usize,
) -> TransactionQueueHandles<T> {
    let (tx, rx) = channel::<TransactionTask<T>>(channel_capacity);
    let (result_tx, result_rx) = channel::<CompletedTransactionTask<T>>(channel_capacity);
    TransactionQueueHandles {
        sender: tx,
        receiver: rx,
        result_sender: result_tx,
        result_receiver: result_rx,
    }
}

pub fn create_transaction_queue<T: Send + Clone + 'static>(
    args: TransactionQueueArgs<T>,
) -> JoinHandle<()> {
    let TransactionQueueArgs {
        rpc_client,
        payer,
        batch_duration,
        ws_url,
        receiver: mut rx,
        result_sender: result_tx,
        max_sol_fee,
    } = args;
    let thread: JoinHandle<()> = tokio::spawn(async move {
        let mut tasks: Vec<TransactionTask<T>> = Vec::new();
        let mut interval = interval(batch_duration);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if tasks.is_empty() {
                        continue;
                    }
                    let rpc_client = rpc_client.clone();
                    let blockhash = rpc_client.get_latest_blockhash_with_commitment(CommitmentConfig::finalized()).await.expect("Failed to get latest blockhash");
                    let tpu_client = TpuClient::new("helium-transaction-queue", rpc_client.clone(), ws_url.as_str(), TpuClientConfig::default()).await.expect("Failed to create TPU client");
                    // Process the collected tasks here
                    let ix_groups: Vec<Vec<Instruction>> = tasks.clone().into_iter().map(|t| t.instructions).collect();
                    let txs = pack_instructions_into_transactions(ix_groups, &payer);
                    let mut with_auto_compute: Vec<(Message, Vec<usize>)> = Vec::new();
                    for (tx, task_ids) in &txs {
                        // This is just a tx with compute ixs. Skip it
                        if tx.len() == 2 {
                            continue;
                        }
                        let (computed, fee) = auto_compute_limit_and_price(&rpc_client, tx.clone(), &[&payer], 1.2, Some(&payer.pubkey()), Some(blockhash.0)).await.unwrap();
                        if fee > max_sol_fee {
                            for task_id in task_ids {
                                result_tx.send(CompletedTransactionTask {
                                    err: Some(TransactionQueueError::FeeTooHigh),
                                    task: tasks[*task_id].clone(),
                                }).await.unwrap();
                            }
                            continue;
                        }
                        with_auto_compute.push((Message::new(&computed, Some(&payer.pubkey())), task_ids.clone()));
                    }
                    if with_auto_compute.is_empty() {
                        continue;
                    }
                    let messages: Vec<Message> = with_auto_compute.iter().map(|(msg, _)| msg.clone()).collect();
                    let results = send_and_confirm_transactions_in_parallel(
                        rpc_client.clone(),
                        Some(tpu_client),
                        &messages,
                        &[&payer],
                        SendAndConfirmConfig {
                            with_spinner: true,
                            resign_txs_count: Some(5),
                        },
                    ).await.expect("Failed to send txs");
                    let mut task_results: std::collections::HashMap<usize, Option<TransactionQueueError>> = std::collections::HashMap::new();
                    for (i, result) in results.iter().enumerate() {
                        for task_id in &txs[i].1 {
                                if let Some(err) = result {
                                    task_results.insert(*task_id, Some(TransactionQueueError::TransactionError(err.clone())));
                                } else if !task_results.contains_key(task_id) {
                                    task_results.insert(*task_id, None);
                                }
                        }
                    }
                    for (task_id, err) in task_results {
                        result_tx.send(CompletedTransactionTask {
                            err,
                            task: tasks[task_id].clone(),
                        }).await.unwrap();
                    }
                    tasks.clear();
                }

                Some(task) = rx.recv() => {
                    tasks.push(task);
                }
            }
        }
    });
    thread
}
