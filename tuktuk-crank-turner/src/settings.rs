use std::{collections::HashSet, path::Path, str::FromStr, time::Duration};

use config::{Config, Environment, File};
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Deserialize)]
pub struct Settings {
    /// RUST_LOG compatible settings string. Default
    /// "ingest=debug,poc_store=info"
    #[serde(default = "default_log")]
    pub log: String,

    #[serde(default = "default_max_retries")]
    pub max_retries: u8,
    pub rpc_url: String,
    pub key_path: String,
    #[serde(default = "default_batch_duration")]
    pub batch_duration: Duration,
    #[serde(default = "default_max_sol_fee")]
    pub max_sol_fee: u64,
    /// Maximum amount the payer's balance is allowed to drop over a simulated transaction,
    /// on top of fees. Cranking should only ever pay the payer, so 0 is correct; raise it
    /// only if you knowingly run tasks that spend from the crank turner's wallet.
    #[serde(default)]
    pub max_sol_balance_drop: u64,
    /// If non-empty, only task queues in this list are cranked. Empty means "all queues",
    /// which means running arbitrary code queued by anyone -- prefer an explicit list.
    #[serde(default)]
    pub allowed_task_queues: Vec<String>,
    /// Task queues to never crank. Applied after `allowed_task_queues`.
    #[serde(default)]
    pub denied_task_queues: Vec<String>,
    pub min_crank_fee: u64,
    #[serde(default = "default_pubsub_repoll")]
    pub pubsub_repoll: Duration,
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
    #[serde(default = "default_recent_attempts_window")]
    pub recent_attempts_window: usize,
    #[serde(default = "default_sender_max_re_sign_count")]
    pub sender_max_re_sign_count: u32,
}

fn default_sender_max_re_sign_count() -> u32 {
    2
}

fn default_max_retries() -> u8 {
    5
}

fn default_recent_attempts_window() -> usize {
    5
}

fn default_metrics_port() -> u16 {
    8080
}

fn default_batch_duration() -> Duration {
    Duration::from_millis(500)
}

fn default_pubsub_repoll() -> Duration {
    Duration::from_secs(30)
}

fn default_max_sol_fee() -> u64 {
    100_000_000
}

fn default_log() -> String {
    "info".to_string()
}

impl Settings {
    /// Load Settings from a given path. Settings are loaded from a given
    /// optional path and can be overriden with environment variables.
    ///
    /// Environment overrides have the same name as the entries in the settings
    /// file in uppercase and prefixed with "QN_". For example
    /// "QN_LOG" will override the log setting. A double underscore distinguishes
    /// subsections in the settings file
    pub fn new<P: AsRef<Path>>(path: Option<P>) -> Result<Self, config::ConfigError> {
        let mut builder = Config::builder();

        if let Some(file) = path {
            // Add optional settings file
            builder = builder
                .add_source(File::with_name(&file.as_ref().to_string_lossy()).required(false));
        }
        // Add in settings from the environment (with a prefix of APP)
        // Eg.. `TUKTUK_DEBUG=1 ./target/app` would set the `debug` key
        let mut settings: Settings = builder
            .add_source(
                Environment::with_prefix("TUKTUK")
                    .separator("__")
                    // Allow the list-valued queue filters to be set from a single env var,
                    // e.g. TUKTUK__ALLOWED_TASK_QUEUES="addr1,addr2"
                    .try_parsing(true)
                    .list_separator(",")
                    .with_list_parse_key("allowed_task_queues")
                    .with_list_parse_key("denied_task_queues"),
            )
            .build()
            .and_then(|config| config.try_deserialize())?;

        // Expand environment variables in key_path (supports both $HOME and ~)
        settings.key_path = shellexpand::full(&settings.key_path)
            .map_err(|e| config::ConfigError::Message(format!("Failed to expand key_path: {}", e)))?
            .into_owned();

        Ok(settings)
    }

    pub fn task_queue_filter(&self) -> Result<TaskQueueFilter, config::ConfigError> {
        let parse = |keys: &[String]| -> Result<Vec<Pubkey>, config::ConfigError> {
            keys.iter()
                .map(|k| {
                    Pubkey::from_str(k).map_err(|e| {
                        config::ConfigError::Message(format!("Invalid task queue pubkey {k}: {e}"))
                    })
                })
                .collect()
        };

        Ok(TaskQueueFilter {
            allowed: parse(&self.allowed_task_queues)?.into_iter().collect(),
            denied: parse(&self.denied_task_queues)?.into_iter().collect(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct TaskQueueFilter {
    /// Empty means every queue is permitted.
    allowed: HashSet<Pubkey>,
    denied: HashSet<Pubkey>,
}

impl TaskQueueFilter {
    pub fn permits(&self, task_queue: &Pubkey) -> bool {
        !self.denied.contains(task_queue)
            && (self.allowed.is_empty() || self.allowed.contains(task_queue))
    }
}
