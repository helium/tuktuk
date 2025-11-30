use std::{path, sync::Arc, time::Duration};

use anyhow::Result;
use cache::{LookupTablesCache, TaskQueueCache, TaskStateCache};
use clap::Parser;
use metrics::{register_custom_metrics, REGISTRY};
use profitability::TaskQueueProfitability;
use settings::Settings;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair, signer::EncodableKey};
use solana_transaction_utils::{
    queue::{create_transaction_queue_handles, TransactionQueueArgs},
    sender::TransactionSender,
};
use task_completion_processor::process_task_completions;
use task_context::TaskContext;
use task_processor::process_tasks;
use task_queue::{create_task_queue, TaskQueueArgs};
use tokio::{sync::mpsc::channel, time::interval};
use tokio_graceful_shutdown::{SubsystemBuilder, Toplevel};
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt};
use transaction::TransactionSenderSubsystem;
use tuktuk_sdk::{
    clock::{track, SYSVAR_CLOCK},
    pubsub_client::PubsubClient,
    watcher::PubsubTracker,
};
use warp::{reject::Rejection, reply::Reply, Filter};
use watchers::{args::WatcherArgs, task_queues::get_and_watch_task_queues};

pub mod cache;
mod metrics;
pub mod profitability;
pub mod settings;
mod sync;
pub mod task_completion_processor;
pub mod task_context;
pub mod task_processor;
pub mod task_queue;
pub mod transaction;
pub mod watchers;

#[derive(Debug, clap::Parser)]
#[clap(version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    /// Optional configuration file to use. If present the toml file at the
    /// given path will be loaded. Environment variables can override the
    /// settings in the given file.
    #[clap(
        short = 'c',
        long,
        default_value = "~/.config/helium/cli/tuktuk-crank-turner/config.toml"
    )]
    pub config: Option<path::PathBuf>,
}

impl Cli {
    fn get_expanded_config_path(&self) -> Option<path::PathBuf> {
        self.config.as_ref().map(|p| {
            shellexpand::full(&p.to_string_lossy())
                .map(|expanded| path::PathBuf::from(expanded.as_ref()))
                .unwrap_or_else(|_| p.clone())
        })
    }
}

async fn metrics_handler() -> Result<impl Reply, Rejection> {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&REGISTRY.gather(), &mut buffer) {
        eprintln!("could not encode custom metrics: {e:?}");
    };
    let mut res = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("custom metrics could not be from_utf8'd: {e:?}");
            String::default()
        }
    };
    buffer.clear();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        eprintln!("could not encode prometheus metrics: {e:?}");
    };
    let res_custom = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("prometheus metrics could not be from_utf8'd: {e:?}");
            String::default()
        }
    };
    buffer.clear();

    res.push_str(&res_custom);
    Ok(res)
}

const PACKED_TX_CHANNEL_CAPACITY: usize = 32;

impl Cli {
    pub async fn run(&self) -> Result<()> {
        register_custom_metrics();
        let settings = Settings::new(self.get_expanded_config_path().as_ref())?;
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(&settings.log))
            .with(tracing_subscriber::fmt::layer().with_span_events(FmtSpan::CLOSE))
            .init();

        let metrics_route = warp::path!("metrics").and_then(metrics_handler);
        tokio::spawn(warp::serve(metrics_route).run(([0, 0, 0, 0], settings.metrics_port)));

        let solana_url = settings.rpc_url.clone();
        let solana_ws_url = solana_url
            .replace("http", "ws")
            .replace("https", "wss")
            .replace("8899", "8900");

        // Create a non-blocking RPC client
        // We can work off of processed accounts because we simulate the next tx before actually
        // sending it.
        let commitment = CommitmentConfig::processed();
        let rpc_client = Arc::new(RpcClient::new_with_commitment(
            solana_url.clone(),
            commitment,
        ));

        // For sending transactions, we need to use confirmed commitment
        let tx_sender_rpc_client = Arc::new(RpcClient::new_with_commitment(
            solana_url.clone(),
            CommitmentConfig::confirmed(),
        ));
        let payer_path = settings.key_path;
        let payer = Arc::new(
            Keypair::read_from_file(payer_path).map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );

        // Create a non-blocking PubSub client
        let (pubsub_client_raw, pubsub_handle, shutdown_sender) =
            PubsubClient::new(&solana_ws_url).await?;
        let pubsub_client = Arc::new(pubsub_client_raw);
        let pubsub_tracker = Arc::new(PubsubTracker::new(
            Arc::clone(&rpc_client),
            pubsub_client.clone(),
            Duration::from_secs(60),
            commitment,
        ));

        let clock_acc = rpc_client.get_account(&SYSVAR_CLOCK).await?;
        let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock_acc.data)?;
        let (now_tx, now_rx) = tokio::sync::watch::channel(clock.unix_timestamp as u64);

        let (tasks, task_queue) = create_task_queue(TaskQueueArgs {
            channel_capacity: 100,
            now: now_rx.clone(),
        })
        .await;
        let task_queue_arc = Arc::new(task_queue);

        let watcher_args = WatcherArgs {
            max_retries: settings.max_retries,
            rpc_client: rpc_client.clone(),
            pubsub_tracker: pubsub_tracker.clone(),
            now: now_rx.clone(),
            task_queue: task_queue_arc.clone(),
            min_crank_fee: settings.min_crank_fee,
        };

        let handles = create_transaction_queue_handles(1000);
        let tx_sender = handles.sender.clone();
        let completion_receiver = handles.result_receiver;

        // Create channels for caches
        let (task_state_sender, task_state_rx) = cache::task_state_channel();
        let (lookup_tables_sender, lookup_tables_rx) = cache::lookup_tables_channel();
        let (task_queues_sender, task_queues_rx) = cache::task_queues_channel();

        // Create caches
        let task_state_cache = TaskStateCache::new(task_state_rx);
        let lookup_tables_cache = LookupTablesCache::new(rpc_client.clone(), lookup_tables_rx);
        let task_queues_cache = TaskQueueCache::new(task_queues_rx);

        let task_context = Arc::new(TaskContext {
            tx_sender,
            task_queue: task_queue_arc.clone(),
            now_rx: now_rx.clone(),
            rpc_client: rpc_client.clone(),
            payer: payer.clone(),
            profitability: Arc::new(TaskQueueProfitability::new(settings.recent_attempts_window)),
            task_state_client: task_state_sender,
            lookup_tables_client: lookup_tables_sender,
            task_queues_client: task_queues_sender.clone(),
        });

        let (packed_tx_sender, packed_tx_receiver) = channel(PACKED_TX_CHANNEL_CAPACITY);
        let sender = TransactionSender::new(
            tx_sender_rpc_client.clone(),
            payer.clone(),
            handles.result_sender.clone(),
            settings.sender_max_re_sign_count,
        )
        .await
        .expect("create sender");

        let pubsub_repoll = settings.pubsub_repoll;
        Toplevel::new(move |top_level| async move {
            top_level.start(SubsystemBuilder::new("transaction-sender", {
                move |handle| {
                    // Spawn the sender task with shutdown signal
                    sender.run(packed_tx_receiver, handle)
                }
            }));
            let watcher_args_clone = watcher_args.clone();
            top_level.start(SubsystemBuilder::new("task-queue-watcher", {
                move |handle| {
                    get_and_watch_task_queues(
                        watcher_args_clone,
                        handle,
                        task_queues_sender.clone(),
                    )
                }
            }));
            let task_context_clone = task_context.clone();
            top_level.start(SubsystemBuilder::new("transaction-queue", {
                move |handle| {
                    TransactionSenderSubsystem::new(TransactionQueueArgs {
                        rpc_client: tx_sender_rpc_client,
                        ws_url: solana_ws_url.clone(),
                        payer,
                        batch_duration: settings.batch_duration,
                        receiver: handles.receiver,
                        result_sender: handles.result_sender,
                        max_sol_fee: settings.max_sol_fee,
                        send_in_parallel: true,
                        packed_tx_sender,
                    })
                    .run(handle)
                }
            }));
            top_level.start(SubsystemBuilder::new("task-processor", {
                move |handle| process_tasks(Box::new(tasks), task_context_clone, handle)
            }));
            let task_context_clone = task_context.clone();
            top_level.start(SubsystemBuilder::new("completion-processor", {
                move |handle| {
                    process_task_completions(completion_receiver, task_context_clone, handle)
                }
            }));
            // Poll RPC for changes to pubsub keys every 30 seconds
            top_level.start(SubsystemBuilder::new("pubsub-tracker", {
                let pubsub_tracker = pubsub_tracker.clone();
                move |handle| async move {
                    let mut interval = interval(pubsub_repoll);
                    loop {
                        tokio::select! {
                            _ = interval.tick() => {
                                if let Err(e) = pubsub_tracker.check_for_changes().await {
                                    tracing::error!("Error checking for changes: {:?}", e);
                                }
                            }
                            _ = handle.on_shutdown_requested() => {
                                tracing::info!("Shutdown requested, exiting pubsub-tracker");
                                break;
                            }
                        }
                    }
                    anyhow::Ok(())
                }
            }));
            top_level.start(SubsystemBuilder::new("clock-tracker", {
                let now_tx = now_tx.clone();
                let pubsub_tracker = pubsub_tracker.clone();
                move |handle| track(now_tx, pubsub_tracker, handle)
            }));
            top_level.start(SubsystemBuilder::new("pubsub-client", {
                move |handle| async move {
                    tokio::select! {
                        _ = handle.on_shutdown_requested() => {
                            tracing::info!("Shutdown requested, exiting pubsub-client");
                            shutdown_sender.send(()).map_err(|_| anyhow::anyhow!("Failed to send shutdown signal"))?;
                            anyhow::Ok(())
                        },
                        res = pubsub_handle => {
                            tracing::info!("Received pubsub message");
                            res.map_err(|e| anyhow::anyhow!(e.to_string()))?
                                  .map_err(|e| anyhow::anyhow!(e.to_string()))
                        }
                    }
                }
            }));
            top_level.start(SubsystemBuilder::new("task-state-cache", {
                move |handle| task_state_cache.run(handle)
            }));
            top_level.start(SubsystemBuilder::new("lookup-tables-cache", {
                move |handle| lookup_tables_cache.run(handle)
            }));
            top_level.start(SubsystemBuilder::new("task-queues-cache", {
                move |handle| task_queues_cache.run(handle)
            }));
        })
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(5000))
        .await
        .map_err(Into::into)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    cli.run().await
}
