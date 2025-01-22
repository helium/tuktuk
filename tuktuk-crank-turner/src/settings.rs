use std::{path::Path, time::Duration};

use config::{Config, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    /// RUST_LOG compatible settings string. Default
    /// "ingest=debug,poc_store=info"
    #[serde(default = "default_log")]
    pub log: String,

    pub max_retries: u8,
    pub rpc_url: String,
    pub key_path: String,
    #[serde(default = "default_batch_duration")]
    pub batch_duration: Duration,
    #[serde(default = "default_max_sol_fee")]
    pub max_sol_fee: u64,
    pub min_crank_fee: u64,
    #[serde(default = "default_pubsub_repoll")]
    pub pubsub_repoll: Duration,
}

fn default_batch_duration() -> Duration {
    Duration::from_secs(2)
}

fn default_pubsub_repoll() -> Duration {
    Duration::from_secs(30)
}

fn default_max_sol_fee() -> u64 {
    100_000_000
}

fn default_log() -> String {
    "queue_node=debug".to_string()
}

impl Settings {
    /// Load Settings from a given path. Settings are loaded from a given
    /// optional path and can be overriden with environment variables.
    ///
    /// Environemnt overrides have the same name as the entries in the settings
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
        builder
            .add_source(Environment::with_prefix("TUKTUK").separator("__"))
            .build()
            .and_then(|config| config.try_deserialize())
    }
}
