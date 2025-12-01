use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use chrono::{Local, TimeZone};
use clap::{Args, Subcommand};
use clock::SYSVAR_CLOCK;
use futures::stream::StreamExt;
use itertools::Itertools;
use serde::Serialize;
use solana_client::{
    rpc_client::GetConfirmedSignaturesForAddress2Config,
    rpc_config::{RpcSimulateTransactionConfig, RpcTransactionConfig},
};
use solana_sdk::{
    commitment_config::CommitmentLevel,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signer::Signer,
    transaction::VersionedTransaction,
};
use solana_transaction_status_client_types::UiTransactionEncoding;
use solana_transaction_utils::{
    pack::pack_instructions_into_transactions, priority_fee::auto_compute_limit_and_price,
};
use tuktuk_program::{
    types::{QueueTaskArgsV0, TriggerV0},
    TaskQueueV0, TaskV0,
};
use tuktuk_sdk::prelude::*;

use super::{task_queue::TaskQueueArg, TransactionSource};
use crate::{
    client::{send_instructions, CliClient},
    cmd::Opts,
    result::Result,
    serde::{print_json, serde_pubkey},
};

#[derive(Debug, Args)]
pub struct TaskCmd {
    #[arg(long, default_value = "false")]
    pub verbose: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    List {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        // Description prefix for the task to filter by
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "false")]
        skip_simulate: bool,
        #[arg(
            long,
            help = "Only show tasks that could be executed now",
            default_value = "false"
        )]
        active: bool,
        #[arg(long, help = "Show tasks with a succesful/failed simulation")]
        successful: Option<bool>,
        #[arg(long, help = "Limit the number of tasks returned")]
        limit: Option<u32>,
    },
    Run {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(short, long)]
        id: Option<u16>,
        // Description prefix to run by
        #[arg(long)]
        description: Option<String>,
        #[arg(short, long, default_value = "false")]
        skip_preflight: bool,
    },
    Requeue {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(short, long)]
        id: Option<u16>,
        #[arg(short, long, default_value = "false", help = "Requeue all stale tasks")]
        stale: bool,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        after_id: Option<u16>,
        #[arg(long)]
        new_timestamp: Option<i64>,
    },
    Close {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(short, long)]
        id: Option<u16>,
        // Description prefix to close by
        #[arg(long)]
        description: Option<String>,
        #[arg(
            long,
            default_value = "false",
            help = "Close tasks that fail simulation"
        )]
        failed: bool,
    },
    Watch {
        #[command(flatten)]
        task_queue: TaskQueueArg,
        #[arg(
            long,
            help = "Description prefix to watch for (can be specified multiple times)"
        )]
        description: Vec<String>,
    },
}

async fn simulate_task(client: &CliClient, task_key: Pubkey) -> Result<Option<SimulationResult>> {
    // Get the run instruction
    let run_ix_res = tuktuk_sdk::compiled_transaction::run_ix(
        client.as_ref(),
        client.as_ref(),
        task_key,
        client.payer.pubkey(),
        &HashSet::new(),
    )
    .await;

    match run_ix_res {
        Ok(run_ix) => {
            // Create and simulate the transaction
            let mut updated_instructions = vec![
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    1900000,
                ),
            ];
            updated_instructions.extend(run_ix.instructions.clone());
            let recent_blockhash = client.rpc_client.get_latest_blockhash().await?;
            let message = VersionedMessage::V0(v0::Message::try_compile(
                &client.payer.pubkey(),
                &updated_instructions,
                &run_ix.lookup_tables,
                recent_blockhash,
            )?);
            let tx = VersionedTransaction::try_new(message, &[&client.payer])?;
            let sim_result = client
                .rpc_client
                .simulate_transaction_with_config(
                    &tx,
                    RpcSimulateTransactionConfig {
                        commitment: Some(
                            solana_sdk::commitment_config::CommitmentConfig::confirmed(),
                        ),
                        sig_verify: true,
                        ..Default::default()
                    },
                )
                .await;

            match sim_result {
                Ok(simulated) => Ok(Some(SimulationResult {
                    error: simulated.value.err.map(|e| e.to_string()),
                    logs: Some(simulated.value.logs.unwrap_or_default()),
                    compute_units: simulated.value.units_consumed,
                })),
                Err(err) => Ok(Some(SimulationResult {
                    error: Some(err.to_string()),
                    logs: None,
                    compute_units: None,
                })),
            }
        }
        Err(tuktuk_sdk::error::Error::AccountNotFound) => Ok(None),
        Err(e) => Ok(Some(SimulationResult {
            error: Some(e.to_string()),
            logs: None,
            compute_units: None,
        })),
    }
}

#[derive(Clone, Serialize)]
struct SimulationResult {
    pub error: Option<String>,
    pub logs: Option<Vec<String>>,
    pub compute_units: Option<u64>,
}

async fn handle_task_completion(client: &CliClient, task_key: Pubkey, task_id: u16) -> Result {
    println!(
        "Task {} completed! Getting transaction signature...",
        task_id
    );

    // Get the last 10 transaction signatures for this task
    let signatures = client
        .rpc_client
        .get_signatures_for_address_with_config(
            &task_key,
            GetConfirmedSignaturesForAddress2Config {
                limit: Some(10),
                ..Default::default()
            },
        )
        .await?;

    if signatures.is_empty() {
        println!("No transaction signature found for task {}", task_id);
        return Ok(());
    }

    // Limit to last 10 transactions
    let recent_signatures: Vec<solana_sdk::signature::Signature> = signatures
        .iter()
        .take(10)
        .map(|sig_info| sig_info.signature.parse().unwrap())
        .collect();

    // Get statuses for all signatures at once
    let signature_statuses = client
        .rpc_client
        .get_signature_statuses_with_history(&recent_signatures)
        .await?;

    // Find the first successful transaction
    let mut successful_signature = None;
    for (i, status_result) in signature_statuses.value.iter().enumerate() {
        match status_result {
            Some(status) => {
                // Check if the transaction was successful (no error)
                if status.err.is_none() {
                    successful_signature = Some(recent_signatures[i].to_string());
                    break;
                }
            }
            None => {
                // Transaction not found, continue to next
                continue;
            }
        }
    }

    if let Some(signature) = successful_signature {
        println!("Successful transaction signature: {}", signature);

        // Get the full transaction to extract logs
        match client
            .rpc_client
            .get_transaction_with_config(
                &signature.parse()?,
                RpcTransactionConfig {
                    encoding: Some(UiTransactionEncoding::Json),
                    max_supported_transaction_version: Some(0),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(tx) => {
                if let Some(meta) = tx.transaction.meta {
                    match meta.log_messages {
                        solana_transaction_status_client_types::option_serializer::OptionSerializer::Some(logs) => {
                            println!("Transaction logs:");
                            for log in logs {
                                println!("  {}", log);
                            }
                        }
                        _ => {
                            println!("No logs found in transaction");
                        }
                    }
                } else {
                    println!("No transaction metadata found");
                }
            }
            Err(e) => {
                println!("Error getting transaction details: {}", e);
            }
        }
    } else {
        println!(
            "No successful transaction found for task {} (all {} recent transactions failed)",
            task_id,
            recent_signatures.len()
        );
    }

    Ok(())
}

impl TaskCmd {
    pub async fn run(&self, opts: Opts) -> Result {
        match &self.cmd {
            Cmd::List {
                task_queue,
                description,
                skip_simulate,
                active,
                limit,
                successful,
            } => {
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();

                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Topic account not found"))?;
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let tasks = client
                    .as_ref()
                    .anchor_accounts::<TaskV0>(&task_keys)
                    .await?;

                let clock_acc = client.rpc_client.get_account(&SYSVAR_CLOCK).await?;
                let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock_acc.data)?;
                let now = clock.unix_timestamp;

                let filtered_tasks = tasks
                    .into_iter()
                    .filter(|(_, task)| {
                        if let Some(task) = task {
                            if let Some(description) = description {
                                if !task.description.starts_with(description) {
                                    return false;
                                }
                            }

                            // If active flag is set, only show tasks that are active
                            // Otherwise, show all tasks
                            return !*active || task.trigger.is_active(now);
                        }
                        false
                    })
                    .collect::<Vec<_>>();

                let mut json_tasks = Vec::with_capacity(filtered_tasks.len());
                let mut simulation_tasks = Vec::new();

                for (i, (pubkey, maybe_task)) in filtered_tasks.into_iter().enumerate() {
                    if let Some(task) = maybe_task {
                        if !*skip_simulate && task.trigger.is_active(now) {
                            simulation_tasks.push((i, pubkey));
                        }

                        json_tasks.push((
                            i,
                            Task {
                                pubkey,
                                id: task.id,
                                description: task.description,
                                trigger: Trigger::from(task.trigger),
                                crank_reward: task.crank_reward,
                                rent_refund: task.rent_refund,
                                simulation_result: None,
                                transaction: if self.verbose {
                                    Some(TransactionSource::from(task.transaction.clone()))
                                } else {
                                    None
                                },
                            },
                        ));

                        if let Some(limit) = limit {
                            if json_tasks.len() >= *limit as usize {
                                break;
                            }
                        }
                    }
                }

                // Run simulations in parallel with a limit of 10 concurrent tasks
                let client = Arc::new(client);
                let simulation_results = futures::stream::iter(simulation_tasks)
                    .map(|(i, pubkey)| {
                        let client = client.clone();
                        async move {
                            let result = simulate_task(&client, pubkey).await;
                            (i, result)
                        }
                    })
                    .buffer_unordered(10)
                    .collect::<Vec<_>>()
                    .await;

                let mut results = vec![None; json_tasks.len()];
                for (i, result) in simulation_results {
                    if let Ok(sim_result) = result {
                        results[i] = sim_result;
                    }
                }

                // Update tasks with simulation results
                for (i, task) in json_tasks.iter_mut() {
                    task.simulation_result = results[*i].clone();
                }

                // Filter by simulation success/failure if requested
                let mut final_tasks = json_tasks
                    .into_iter()
                    .map(|(_, task)| task)
                    .collect::<Vec<_>>();
                if let Some(successful) = successful {
                    final_tasks.retain(|task| {
                        if let Some(simulation_result) = &task.simulation_result {
                            (*successful && simulation_result.error.is_none())
                                || (!*successful && simulation_result.error.is_some())
                        } else {
                            !*successful
                        }
                    });
                }

                print_json(&final_tasks)?;
            }
            Cmd::Close {
                task_queue,
                id: index,
                description,
                failed,
            } => {
                if index.is_none() && description.is_none() {
                    return Err(anyhow!("Either id or description must be provided"));
                }
                if index.is_some() && description.is_some() {
                    return Err(anyhow!("Only one of id or description can be provided"));
                }
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();
                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Task queue account not found"))?;
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let tasks = if let Some(index) = index {
                    let task_key = tuktuk::task::key(&task_queue_pubkey, *index);
                    let task = client
                        .as_ref()
                        .anchor_account::<TaskV0>(&task_key)
                        .await?
                        .ok_or_else(|| anyhow!("Task not found"))?;
                    vec![(task_key, task)]
                } else if let Some(description) = description {
                    let tasks = client
                        .as_ref()
                        .anchor_accounts::<TaskV0>(&task_keys)
                        .await?;
                    let clock_acc = client.rpc_client.get_account(&SYSVAR_CLOCK).await?;
                    let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock_acc.data)?;
                    let now = clock.unix_timestamp;
                    tasks
                        .into_iter()
                        .filter(|(_, task)| {
                            if let Some(task) = task {
                                if *failed && !task.trigger.is_active(now) {
                                    return false;
                                }
                                return task.description.starts_with(description);
                            }

                            false
                        })
                        .map(|(p, task)| (p, task.unwrap().clone()))
                        .collect()
                } else {
                    vec![]
                };

                let mut seen_ids = HashSet::new();
                let mut to_close = Vec::new();

                // If failed flag is set, simulate each task first
                let client = Arc::new(client);
                let simulation_tasks = tasks
                    .iter()
                    .filter(|(_, task)| seen_ids.insert(task.id))
                    .map(|(pubkey, _)| *pubkey)
                    .collect::<Vec<_>>();

                let mut simulation_results = HashMap::new();
                if *failed {
                    // Run simulations in parallel with a limit of 10 concurrent tasks
                    let results = futures::stream::iter(simulation_tasks)
                        .map(|pubkey| {
                            let client = client.clone();
                            async move {
                                let result = simulate_task(&client, pubkey).await;
                                (pubkey, result)
                            }
                        })
                        .buffer_unordered(10)
                        .collect::<Vec<_>>()
                        .await;

                    // Collect results into a HashMap for O(1) lookups
                    simulation_results = results.into_iter().collect();
                }

                // Filter tasks based on simulation results if failed flag is set
                for (pubkey, task) in &tasks {
                    if seen_ids.contains(&task.id) {
                        if *failed {
                            if let Some(Ok(Some(sim_result))) = simulation_results.get(pubkey) {
                                if sim_result.error.is_some() {
                                    to_close.push(task.clone());
                                }
                            }
                        } else {
                            to_close.push(task.clone());
                        }
                    }
                }

                let ixs = to_close
                    .into_iter()
                    .map(|task| {
                        tuktuk::task::dequeue_ix(
                            task_queue_pubkey,
                            client.payer.pubkey(),
                            task.rent_refund,
                            task.id,
                        )
                        .map_err(|e| anyhow!("Failed to dequeue task: {}", e))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let ix_groups = ixs.into_iter().map(|ix| vec![ix]).collect_vec();
                let groups = pack_instructions_into_transactions(
                    &ix_groups.iter().map(|ix| ix.as_slice()).collect_vec(),
                    None,
                )?;

                for mut to_send in groups {
                    // Remove compute budget ixs
                    to_send.instructions.remove(0);
                    to_send.instructions.remove(0);
                    send_instructions(
                        client.rpc_client.clone(),
                        &client.payer,
                        client.opts.ws_url().as_str(),
                        &to_send.instructions,
                        &[],
                    )
                    .await?;
                }
            }
            Cmd::Run {
                task_queue,
                id,
                skip_preflight,
                description,
            } => {
                if id.is_none() && description.is_none() {
                    return Err(anyhow!("Either id or description must be provided"));
                }
                if id.is_some() && description.is_some() {
                    return Err(anyhow!("Only one of id or description can be provided"));
                }
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();
                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Task queue account not found"))?;
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let tasks = if let Some(id) = id {
                    let task_key = tuktuk::task::key(&task_queue_pubkey, *id);
                    let task = client
                        .as_ref()
                        .anchor_account::<TaskV0>(&task_key)
                        .await?
                        .ok_or_else(|| anyhow!("Task not found"))?;
                    vec![(task_key, task)]
                } else if let Some(description) = description {
                    let tasks = client
                        .as_ref()
                        .anchor_accounts::<TaskV0>(&task_keys)
                        .await?;
                    tasks
                        .into_iter()
                        .filter(|(_, task)| {
                            if let Some(task) = task {
                                return task.description.starts_with(description);
                            }
                            false
                        })
                        .map(|(p, task)| (p, task.unwrap().clone()))
                        .collect()
                } else {
                    vec![]
                };
                for (task_key, _) in tasks {
                    let run_ix_result = tuktuk_sdk::compiled_transaction::run_ix(
                        client.as_ref(),
                        client.as_ref(),
                        task_key,
                        client.payer.pubkey(),
                        &HashSet::new(),
                    )
                    .await;
                    match run_ix_result {
                        Ok(run_ix) => {
                            let blockhash = client.rpc_client.get_latest_blockhash().await?;
                            let (computed, _) = auto_compute_limit_and_price(
                                &client.rpc_client,
                                &run_ix.instructions,
                                1.2,
                                &client.payer.pubkey(),
                                Some(blockhash),
                                Some(run_ix.lookup_tables.clone()),
                            )
                            .await
                            .unwrap();

                            let recent_blockhash = client.rpc_client.get_latest_blockhash().await?;
                            let message = VersionedMessage::V0(v0::Message::try_compile(
                                &client.payer.pubkey(),
                                &computed,
                                &run_ix.lookup_tables,
                                recent_blockhash,
                            )?);
                            let tx = VersionedTransaction::try_new(message, &[&client.payer])?;
                            let txid = client
                                .rpc_client
                                .send_transaction_with_config(
                                    &tx,
                                    solana_client::rpc_config::RpcSendTransactionConfig {
                                        skip_preflight: *skip_preflight,
                                        preflight_commitment: Some(CommitmentLevel::Confirmed),
                                        ..Default::default()
                                    },
                                )
                                .await?;

                            println!("Tx sent: {txid}");
                        }
                        Err(e) => {
                            println!("Error running task: {e:?}");
                        }
                    }
                }
            }
            Cmd::Requeue {
                task_queue,
                id,
                new_timestamp,
                stale,
                description,
                after_id,
            } => {
                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();
                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Topic account not found"))?;
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let tasks = client
                    .as_ref()
                    .anchor_accounts::<TaskV0>(&task_keys)
                    .await?;

                let clock_acc = client.rpc_client.get_account(&SYSVAR_CLOCK).await?;
                let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock_acc.data)?;
                let now = clock.unix_timestamp;

                let filtered_tasks = tasks.into_iter().filter(|(_, task)| {
                    if let Some(task) = task {
                        if *stale {
                            let is_stale = task.trigger.is_active(now)
                                && match task.trigger {
                                    TriggerV0::Now => false,
                                    TriggerV0::Timestamp(ts) => {
                                        now - ts > task_queue.stale_task_age as i64
                                    }
                                };

                            if !is_stale {
                                return false;
                            }
                        }

                        if let Some(description) = description {
                            if !task.description.starts_with(description) {
                                return false;
                            }
                        }

                        if let Some(after_id) = after_id {
                            if task.id <= *after_id {
                                return false;
                            }
                        }

                        if let Some(id) = id {
                            if task.id != *id {
                                return false;
                            }
                        }

                        return true;
                    }
                    false
                });

                let collected_tasks = filtered_tasks
                    .into_iter()
                    .flat_map(|(_, task)| task)
                    .collect_vec();

                println!("Requeueing {} tasks", collected_tasks.len());

                for task in collected_tasks {
                    let (new_task_key, ix) = tuktuk::task::queue(
                        client.as_ref(),
                        client.payer.pubkey(),
                        client.payer.pubkey(),
                        task_queue_pubkey,
                        QueueTaskArgsV0 {
                            id: task.id,
                            trigger: new_timestamp.map_or(TriggerV0::Now, TriggerV0::Timestamp),
                            transaction: task.transaction.clone(),
                            crank_reward: Some(task.crank_reward),
                            free_tasks: task.free_tasks,
                            description: task.description,
                        },
                    )
                    .await?;

                    send_instructions(
                        client.rpc_client.clone(),
                        &client.payer,
                        client.opts.ws_url().as_str(),
                        &[ix],
                        &[],
                    )
                    .await?;

                    println!("New task key: {new_task_key}");
                }
            }
            Cmd::Watch {
                task_queue,
                description,
            } => {
                if description.is_empty() {
                    return Err(anyhow!(
                        "At least one description must be provided for watch command"
                    ));
                }

                let client = opts.client().await?;
                let task_queue_pubkey = task_queue.get_pubkey(&client).await?.unwrap();
                let task_queue: TaskQueueV0 = client
                    .as_ref()
                    .anchor_account(&task_queue_pubkey)
                    .await?
                    .ok_or_else(|| anyhow!("Task queue account not found"))?;

                let trimmed_descriptions: Vec<String> = description
                    .iter()
                    .map(|prefix| {
                        if prefix.len() > 40 {
                            prefix.chars().take(40).collect()
                        } else {
                            prefix.clone()
                        }
                    })
                    .collect();

                // First, get and display all existing tasks that match the description prefixes
                let task_keys = tuktuk::task::keys(&task_queue_pubkey, &task_queue)?;
                let existing_tasks = client
                    .as_ref()
                    .anchor_accounts::<TaskV0>(&task_keys)
                    .await?;

                let mut watched_tasks = std::collections::HashMap::new();

                // Filter and start watching existing tasks that match our prefixes
                for (task_key, maybe_task) in existing_tasks {
                    if let Some(task) = maybe_task {
                        // Check if task description matches any of the prefixes
                        let matches = trimmed_descriptions
                            .iter()
                            .any(|prefix| task.description.starts_with(prefix));
                        if matches {
                            println!(
                                "Found existing matching task: {} (ID: {}, KEY: {})",
                                task.description, task.id, task_key
                            );
                            watched_tasks.insert(task_key, task.id);
                        }
                    }
                }

                // Set up pubsub tracker for watching
                let (pubsub_client_raw, _pubsub_handle, _shutdown_sender) =
                    tuktuk_sdk::pubsub_client::PubsubClient::new(client.opts.ws_url().as_str())
                        .await?;
                let pubsub_client = Arc::new(pubsub_client_raw);
                let pubsub_tracker = Arc::new(tuktuk_sdk::watcher::PubsubTracker::new(
                    client.rpc_client.clone(),
                    pubsub_client,
                    Duration::from_secs(30),
                    solana_sdk::commitment_config::CommitmentConfig::confirmed(),
                ));

                // Start watching for task updates
                let (stream, _unsub) = tuktuk::task::on_new(
                    client.as_ref(),
                    &pubsub_tracker,
                    &task_queue_pubkey,
                    &task_queue,
                )
                .await?;
                println!(
                    "Watching for tasks with description prefixes: {:?}",
                    trimmed_descriptions
                );
                println!("Press Ctrl+C to stop watching...");

                let mut stream = Box::pin(stream);

                while let Some(update) = stream.next().await {
                    match update {
                        Ok(task_update) => {
                            // Check for new tasks that match any of our descriptions
                            for (task_key, maybe_task) in task_update.tasks {
                                if let Some(task) = maybe_task {
                                    // Check if task description matches any of the prefixes
                                    let matches = trimmed_descriptions
                                        .iter()
                                        .any(|prefix| task.description.starts_with(prefix));
                                    if matches {
                                        println!(
                                            "Found matching task: {} (ID: {}, KEY: {})",
                                            task.description, task.id, task_key
                                        );
                                        watched_tasks.insert(task_key, task.id);
                                    }
                                } else {
                                    // Task was removed (completed)
                                    if let Some(task_id) = watched_tasks.remove(&task_key) {
                                        if let Err(e) =
                                            handle_task_completion(&client, task_key, task_id).await
                                        {
                                            eprintln!("Error handling task completion: {}", e);
                                        }
                                    }
                                }
                            }

                            // Check for removed tasks
                            for removed_task_key in task_update.removed {
                                if let Some(task_id) = watched_tasks.remove(&removed_task_key) {
                                    if let Err(e) =
                                        handle_task_completion(&client, removed_task_key, task_id)
                                            .await
                                    {
                                        eprintln!("Error handling task completion: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error receiving task update: {}", e);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct Task {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    pub id: u16,
    pub description: String,
    #[serde(with = "serde_pubkey")]
    pub rent_refund: Pubkey,
    pub trigger: Trigger,
    pub crank_reward: u64,
    pub simulation_result: Option<SimulationResult>,
    pub transaction: Option<TransactionSource>,
}

#[derive(Serialize)]
enum Trigger {
    Now,
    Timestamp {
        epoch: i64,
        #[serde(rename = "human_readable")]
        formatted: String,
    },
}

impl From<TriggerV0> for Trigger {
    fn from(trigger: TriggerV0) -> Self {
        match trigger {
            TriggerV0::Now => Trigger::Now,
            TriggerV0::Timestamp(ts) => Trigger::Timestamp {
                epoch: ts,
                formatted: Local
                    .timestamp_opt(ts, 0)
                    .single()
                    .unwrap_or_else(Local::now)
                    .to_rfc3339(),
            },
        }
    }
}
