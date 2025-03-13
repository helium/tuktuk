use std::{collections::HashMap, sync::Arc, time::Duration};

use solana_client::{
    nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
    tpu_client::TpuClientConfig,
};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    commitment_config::{CommitmentConfig, CommitmentLevel},
    instruction::Instruction,
    message::v0,
    signature::Keypair,
    signer::Signer,
    transaction::TransactionError,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinHandle,
    time::interval,
};

use crate::{
    pack::pack_instructions_into_transactions,
    priority_fee::auto_compute_limit_and_price,
    send_and_confirm_transactions_in_parallel::{
        send_and_confirm_transactions_in_parallel, SendAndConfirmConfig,
    },
};

#[derive(Debug, Clone)]
pub struct TransactionTask<T: Send + Clone> {
    pub task: T,
    // What is this task worth in lamports? Will not run tx if it is not worth it. To guarentee task runs, set to u64::MAX
    pub worth: u64,
    pub instructions: Vec<Instruction>,
    pub lookup_tables: Option<Vec<AddressLookupTableAccount>>,
}

#[derive(Debug, thiserror::Error, Clone)]
pub enum TransactionQueueError {
    #[error("Transaction error: {0}")]
    TransactionError(#[from] TransactionError),
    #[error("Raw transaction error: {0}")]
    RawTransactionError(String),
    #[error("Raw simulated transaction error: {0}")]
    RawSimulatedTransactionError(String),
    #[error("Simulated transaction error: {0}")]
    SimulatedTransactionError(TransactionError),
    #[error("Fee too high")]
    FeeTooHigh,
    #[error("Ix group too large")]
    IxGroupTooLarge,
    #[error("Tpu sender error: {0}")]
    TpuSenderError(String),
}

#[derive(Debug)]
pub struct CompletedTransactionTask<T: Send + Clone> {
    pub err: Option<TransactionQueueError>,
    pub fee: u64,
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
    pub send_in_parallel: bool,
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

pub fn create_transaction_queue<T: Send + Clone + 'static + Sync>(
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
        send_in_parallel,
    } = args;
    let payer_pubkey = payer.pubkey();
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
                    let lookup_tables: Vec<AddressLookupTableAccount> = {
                        let mut seen = std::collections::HashSet::new();
                        tasks.clone()
                            .into_iter()
                            .flat_map(|t| t.lookup_tables.unwrap_or_default())
                            .filter(|table| seen.insert(table.key))  // Only keep tables whose addresses we haven't seen
                            .collect()
                    };

                    let txs = pack_instructions_into_transactions(ix_groups, &payer, Some(lookup_tables.clone()));
                    let mut fees_by_task_id: HashMap<usize, u64> = HashMap::new();
                    match txs {
                        Ok(txs) => {
                            let mut with_auto_compute: Vec<(v0::Message, Vec<usize>)> = Vec::new();
                            for tx in &txs {
                                // This is just a tx with compute ixs. Skip it
                                if tx.is_empty() {
                                    continue;
                                }
                                let (computed, fee) = auto_compute_limit_and_price(&rpc_client, tx.instructions.clone(), 1.2, &payer_pubkey, Some(blockhash.0), Some(lookup_tables.clone())).await.unwrap();
                                let num_tasks = tx.task_ids.len();
                                for task_id in tx.task_ids.iter() {
                                    fees_by_task_id.insert(*task_id, fee.div_ceil(num_tasks as u64));
                                }
                                let total_task_worth = tx.task_ids.iter().map(|task_id| tasks[*task_id].worth).sum::<u64>();
                                if fee > max_sol_fee || fee > total_task_worth {
                                    for task_id in &tx.task_ids {
                                        result_tx.send(CompletedTransactionTask {
                                            err: Some(TransactionQueueError::FeeTooHigh),
                                            task: tasks[*task_id].clone(),
                                            fee: 0,
                                        }).await.unwrap();
                                    }
                                    continue;
                                }
                                with_auto_compute.push((v0::Message::try_compile(
                                    &payer_pubkey,
                                    &computed,
                                    &lookup_tables.clone(),
                                    blockhash.0,
                                ).unwrap(), tx.task_ids.clone()));
                            }
                            if with_auto_compute.is_empty() {
                                continue;
                            }
                            let messages: Vec<v0::Message> = with_auto_compute.iter().map(|(msg, _)| msg.clone()).collect();
                            let mut task_results: std::collections::HashMap<usize, Option<TransactionQueueError>> = std::collections::HashMap::new();
                            if !send_in_parallel {
                                // Send transactions without confirmation
                                for (i, message) in messages.iter().enumerate() {
                                    let mut tx = solana_sdk::transaction::VersionedTransaction::try_new(
                                            solana_sdk::message::VersionedMessage::V0(message.clone()),
                                            &[&payer],
                                        ).unwrap();
                                    tx.message.set_recent_blockhash(blockhash.0);
                                    match rpc_client.send_transaction_with_config(
                                        &tx,
                                        solana_client::rpc_config::RpcSendTransactionConfig {
                                            skip_preflight: false,
                                            preflight_commitment: Some(CommitmentLevel::Confirmed),
                                            ..Default::default()
                                        },
                                    ).await {
                                        Ok(_) => {
                                            for task_id in &with_auto_compute[i].1 {
                                                task_results.insert(*task_id, None);
                                            }
                                        }
                                        Err(err) => {
                                            for task_id in &with_auto_compute[i].1 {
                                                task_results.insert(*task_id, Some(TransactionQueueError::RawTransactionError(
                                                    err.to_string()
                                                )));
                                            }
                                        }
                                    }
                                }
                            } else {
                                let maybe_results = send_and_confirm_transactions_in_parallel(
                                    rpc_client.clone(),
                                    Some(tpu_client),
                                    &messages,
                                    &[&payer],
                                    SendAndConfirmConfig {
                                        with_spinner: true,
                                        resign_txs_count: Some(5),
                                    },
                                ).await;
                                match maybe_results {
                                    Ok(results) => {
                                        for (i, result) in results.iter().enumerate() {
                                            for task_id in &txs[i].task_ids {
                                                    if let Some(err) = result {
                                                        task_results.insert(*task_id, Some(TransactionQueueError::TransactionError(err.clone())));
                                                    } else if !task_results.contains_key(task_id) {
                                                        task_results.insert(*task_id, None);
                                                    }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let e = TransactionQueueError::TpuSenderError(e.to_string());
                                        for task in tasks.iter() {
                                            result_tx.send(CompletedTransactionTask {
                                                err: Some(e.clone()),
                                                fee: 0,
                                                task: task.clone(),
                                            }).await.unwrap();
                                        }
                                    }
                                }

                            }
                            for (task_id, err) in task_results {
                                    result_tx.send(CompletedTransactionTask {
                                        err,
                                        task: tasks[task_id].clone(),
                                        fee: fees_by_task_id[&task_id],
                                    }).await.unwrap();
                                }
                            tasks.clear();
                        }
                        Err(_) => {
                            for task in tasks.iter() {
                                result_tx.send(CompletedTransactionTask {
                                    err: Some(TransactionQueueError::IxGroupTooLarge),
                                    task: task.clone(),
                                    fee: 0,
                                }).await.unwrap();
                            }
                            tasks.clear();
                        }
                    }

                }

                Some(task) = rx.recv() => {
                    tasks.push(task);
                }
            }
        }
    });
    thread
}
