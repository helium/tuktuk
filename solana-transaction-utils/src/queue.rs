use std::{sync::Arc, time::Duration};

use futures::stream::{FuturesUnordered, StreamExt};
use itertools::Itertools;
use solana_account_decoder_client_types::UiAccountEncoding;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig},
};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    signature::Keypair,
    signer::Signer,
    transaction::{TransactionError, VersionedTransaction},
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    time::{interval, Interval},
};
use tracing::info;

use crate::{
    error::Error,
    pack::{PackedTransaction, MAX_TRANSACTION_SIZE},
    priority_fee::{auto_compute_price, compute_budget_instruction},
    sender::PackedTransactionWithTasks,
};

#[derive(Debug, Clone)]
pub struct TransactionTask<T: Send + Clone> {
    pub task: T,
    // What is this task worth in lamports? Will not run tx if it is not worth it. To guarentee task runs, set to u64::MAX
    pub worth: u64,
    pub instructions: Vec<Instruction>,
    pub lookup_tables: Option<Vec<AddressLookupTableAccount>>,
}

#[derive(Debug)]
pub struct CompletedTransactionTask<T: Send + Clone> {
    pub err: Option<Error>,
    pub fee: u64,
    pub task: TransactionTask<T>,
}

struct SimulationOutcome {
    instructions: Vec<Instruction>,
    err: Option<TransactionError>,
    fee: u64,
    /// How many lamports the payer lost over the simulated transaction, excluding the tx
    /// fee (simulation debits the fee like real execution, so the expected fee is
    /// subtracted back out). 0 if the payer came out even or ahead.
    payer_balance_drop: u64,
}

pub struct TransactionQueueArgs<T: Send + Clone> {
    pub rpc_client: Arc<RpcClient>,
    pub ws_url: String,
    pub payer: Arc<Keypair>,
    pub batch_duration: Duration,
    pub receiver: Receiver<TransactionTask<T>>,
    pub result_sender: Sender<CompletedTransactionTask<T>>,
    pub packed_tx_sender: Sender<PackedTransactionWithTasks<T>>,
    pub max_sol_fee: u64,
    /// Maximum lamports the payer may lose over a simulated transaction, on top of fees.
    pub max_sol_balance_drop: u64,
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

const MAX_PACKABLE_TX_SIZE: usize = 800;
const LAMPORTS_PER_SIGNATURE: u64 = 5_000;

pub async fn create_transaction_queue<T: Send + Clone + 'static + Sync>(
    args: TransactionQueueArgs<T>,
) -> Result<(), Error> {
    let mut receiver = args.receiver;

    // The currently staged bundle of tasks
    let mut bundle = TaskBundle::new();
    // The timer to wait for the batch duration if no new packable tasks show up
    let mut wait_timer: Option<Interval> = None;
    // The queue of tasks currently being simulated waiting to be sent to the sender
    let mut simulation_queue = FuturesUnordered::new();

    async fn simulate_transaction<T: Send + Clone>(
        bundle: TaskBundle<T>,
        rpc_client: Arc<RpcClient>,
        payer: Arc<Keypair>,
    ) -> (Vec<TransactionTask<T>>, Result<SimulationOutcome, Error>) {
        let tasks = bundle.tasks;
        let result = async {
            let payer_pubkey = payer.pubkey();
            let (blockhash, payer_balance_response) = tokio::try_join!(
                rpc_client.get_latest_blockhash(),
                rpc_client.get_balance_with_commitment(&payer_pubkey, rpc_client.commitment())
            )?;
            let payer_balance_before = payer_balance_response.value;
            let message = v0::Message::try_compile(
                &payer.pubkey(),
                &bundle.tx.instructions,
                &bundle.lookup_tables,
                blockhash,
            )?;

            // The base fee is deterministic (5000 lamports per required signature), so
            // compute it locally rather than spending an RPC round-trip on it.
            let expected_fee =
                LAMPORTS_PER_SIGNATURE * message.header.num_required_signatures as u64;
            let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&*payer])
                .map_err(Error::signer)?;
            let sim_result = rpc_client
                .simulate_transaction_with_config(
                    &tx,
                    RpcSimulateTransactionConfig {
                        // Ask for the payer's post-simulation state so we can detect a
                        // transaction that drains us.
                        accounts: Some(RpcSimulateTransactionAccountsConfig {
                            addresses: vec![payer.pubkey().to_string()],
                            encoding: Some(UiAccountEncoding::Base64),
                        }),
                        commitment: Some(rpc_client.commitment()),
                        // Simulate against a bank at least as fresh as the balance snapshot,
                        // otherwise another of our bundles landing in between manufactures a
                        // phantom balance drop.
                        min_context_slot: Some(payer_balance_response.context.slot),
                        ..Default::default()
                    },
                )
                .await?;

            if let Some(ref err) = sim_result.value.err {
                info!(?err, ?sim_result.value.logs, "simulation error");
            }

            // Saturating: a balance increase (the normal case, we get paid) is a drop of 0.
            // Simulation debits the tx fee from the payer just like real execution, so
            // subtract the expected fee to isolate what the instructions themselves take.
            let payer_balance_drop = sim_result
                .value
                .accounts
                .as_ref()
                .and_then(|accounts| accounts.first())
                .and_then(|account| account.as_ref())
                .map(|account| {
                    payer_balance_before
                        .saturating_sub(account.lamports)
                        .saturating_sub(expected_fee)
                })
                .unwrap_or(0);

            // Scale up by 1.2 just to be sure it'll succeed.
            let compute_units =
                (sim_result.value.units_consumed.unwrap_or(1000000) as f64 * 1.2) as u32;
            let mut updated_instructions = bundle.tx.instructions.clone();
            let compute_budget_ix = compute_budget_instruction(compute_units);
            // Replace or insert compute budget instruction
            if let Some(pos) = updated_instructions.iter().position(|ix| {
                ix.program_id == solana_sdk::compute_budget::id()
                    && ix.data.first() == compute_budget_ix.data.first()
            }) {
                updated_instructions[pos] = compute_budget_ix; // Replace existing
            } else {
                updated_instructions.insert(0, compute_budget_ix); // Insert at the beginning
            }

            let fee = if sim_result.value.err.is_some() {
                0
            } else {
                let (ixs, fee) = auto_compute_price(
                    &rpc_client,
                    &updated_instructions,
                    &payer.pubkey(),
                    compute_units,
                )
                .await?;
                updated_instructions = ixs;
                fee
            };

            Ok(SimulationOutcome {
                instructions: updated_instructions,
                err: sim_result.value.err,
                fee,
                payer_balance_drop,
            })
        }
        .await;

        (tasks, result)
    }

    // Main loop with shutdown handling
    loop {
        tokio::select! {
            // If we have a bundle waiting to be packed further and the timer runs out, send it to the sender
            _ = async { if let Some(timer) = &mut wait_timer { timer.tick().await } else { std::future::pending().await } } => {
                if !bundle.is_empty() {
                    simulation_queue.push(simulate_transaction(
                        bundle,
                        args.rpc_client.clone(),
                        args.payer.clone(),
                    ));
                    bundle = TaskBundle::new();
                    wait_timer = None;
                }
            }

            // If we have a new task, try to add it to the bundle
            Some(task) = receiver.recv() => {
                match bundle.add_task(task.clone()) {
                    // Task is small, we can pack it further. Set the timer to wait for the batch duration
                    Ok((len, added)) if added && len <= MAX_PACKABLE_TX_SIZE => {
                        if wait_timer.is_none() {
                            wait_timer = Some(interval(args.batch_duration));
                        }
                    }
                    Ok((_, added)) if added => {
                        // Bundle full, simulate it
                        simulation_queue.push(simulate_transaction(
                            bundle,
                            args.rpc_client.clone(),
                            args.payer.clone(),
                        ));
                        bundle = TaskBundle::new();
                    }
                    Ok((_, added)) if !added => {
                        // Current task won't fit, simulate current bundle first
                        if !bundle.is_empty() {
                            simulation_queue.push(simulate_transaction(
                                bundle,
                                args.rpc_client.clone(),
                                args.payer.clone(),
                            ));
                            bundle = TaskBundle::new();
                        }
                        // Try adding task to empty bundle
                        if let Err(e) = bundle.add_task(task.clone()) {
                            args.result_sender
                                .send(CompletedTransactionTask {
                                    err: Some(e),
                                    task,
                                    fee: 0,
                                })
                                .await?;
                        }
                    }
                    Err(e) => {
                        args.result_sender.send(CompletedTransactionTask {
                            err: Some(e),
                            task,
                            fee: 0,
                        }).await?;
                    },
                    _ => {
                        // We should never get here
                        panic!("Invalid return value from bundle.add_task");
                    }
                }
            }

            Some((tasks, result)) = simulation_queue.next() => {
                match result {
                    Ok(SimulationOutcome { instructions, err: error, fee, payer_balance_drop }) => {
                        // Notify tasks they failed
                        if let Some(e) = error {
                            match e {
                                TransactionError::InstructionError(failed_ix, _) => {
                                    let failed_task_idx = {
                                        let mut current_task: usize = 0;
                                        let mut current_ix: usize = 2; // Skip compute budget instructions

                                        // Find which task contains the failed instruction
                                        while current_ix < failed_ix as usize {
                                            if current_task >= tasks.len() {
                                                break;
                                            }
                                            current_ix += tasks[current_task].instructions.len();
                                            if current_ix < failed_ix as usize {
                                                current_task += 1;
                                            }
                                        }
                                        current_task
                                    };

                                    if failed_task_idx >= tasks.len() {
                                        println!("Failed task index out of bounds {:?} failed_ix: {:?}, tasks lens: {:?}", failed_task_idx, failed_ix, tasks.iter().map(|t| t.instructions.len()).collect_vec());
                                        for task in tasks {
                                            args.result_sender.send(CompletedTransactionTask {
                                                err: Some(Error::SimulatedTransactionError(e.clone())),
                                                task,
                                                fee: 0,
                                            }).await?;
                                        }
                                    } else {
                                        // Handle failed task
                                        args.result_sender.send(CompletedTransactionTask {
                                            err: Some(Error::SimulatedTransactionError(e)),
                                            task: tasks[failed_task_idx].clone(),
                                            fee: 0,
                                        }).await?;

                                        // Requeue remaining tasks
                                        let mut new_bundle = TaskBundle::new();
                                        for (i, task) in tasks.iter().enumerate() {
                                            if i != failed_task_idx {
                                                new_bundle.add_task(task.clone()).expect("add task");
                                            }
                                        }
                                        if !new_bundle.is_empty() {
                                            simulation_queue.push(simulate_transaction(
                                                new_bundle,
                                                args.rpc_client.clone(),
                                                args.payer.clone(),
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    // Other errors affect all tasks
                                    for task in tasks {
                                        args.result_sender.send(CompletedTransactionTask {
                                            err: Some(Error::SimulatedTransactionError(e.clone())),
                                            task,
                                            fee: 0,
                                        }).await?;
                                    }
                                }
                            }
                        } else if payer_balance_drop > args.max_sol_balance_drop {
                            // The transaction takes lamports off the payer beyond fees. Cranking
                            // should only ever pay us, so this is a task trying to spend our money.
                            info!(payer_balance_drop, max = args.max_sol_balance_drop, "rejecting bundle, payer balance drop too high");
                            for task in tasks {
                                args.result_sender.send(CompletedTransactionTask {
                                    err: Some(Error::PayerBalanceDropTooHigh {
                                        drop: payer_balance_drop,
                                        max: args.max_sol_balance_drop,
                                    }),
                                    task,
                                    fee: 0,
                                }).await?;
                            }
                        } else if fee > args.max_sol_fee || fee > tasks.iter().map(|t| t.worth).sum::<u64>() {
                            // Fee too high, notify tasks
                            for task in tasks {
                                args.result_sender.send(CompletedTransactionTask {
                                    err: Some(Error::FeeTooHigh),
                                    task,
                                    fee: 0,
                                }).await?;
                            }
                        } else {
                            // Simulation successful, send to transaction sender
                            args.packed_tx_sender.send(PackedTransactionWithTasks {
                                instructions,
                                tasks,
                                fee,
                                re_sign_count: 0,
                            }).await?;
                        }
                    }
                    Err(e) => {
                        // Simulation failed, notify tasks
                        for task in tasks.iter() {
                            args.result_sender.send(CompletedTransactionTask {
                                err: Some(Error::RawSimulatedTransactionError(e.to_string())),
                                task: task.clone(),
                                fee: 0,
                            }).await?;
                        }
                    }
                }
            }
        }
    }
}

struct TaskBundle<T: Send + Clone> {
    tx: PackedTransaction,
    tasks: Vec<TransactionTask<T>>,
    lookup_tables: Vec<AddressLookupTableAccount>,
}

impl<T: Send + Clone> TaskBundle<T> {
    fn new() -> Self {
        Self {
            tx: PackedTransaction::default(),
            tasks: Vec::new(),
            lookup_tables: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.tx.is_empty()
    }

    // Returns the length of the transaction and a boolean indicating if the task was added
    fn add_task(&mut self, task: TransactionTask<T>) -> Result<(usize, bool), Error> {
        let task_instructions = task.instructions.as_slice();
        let mut test_luts = self.lookup_tables.clone();
        if let Some(luts) = task.lookup_tables.clone() {
            test_luts.extend(luts);
        }

        // Test if we can fit this task
        let len = self.tx.transaction_len(task_instructions, &test_luts)?;

        let mut added = false;
        // Only add the task if it fits
        if len <= MAX_TRANSACTION_SIZE {
            added = true;
            self.lookup_tables = test_luts;
            self.tx.push(task_instructions, 0);
            self.tasks.push(task);
        }

        Ok((len, added))
    }
}
