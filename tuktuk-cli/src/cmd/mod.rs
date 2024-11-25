use std::path::PathBuf;

use clap::Args;
use dirs::home_dir;
use solana_sdk::{signature::Keypair, signer::EncodableKey};

use crate::{client::CliClient, result::Result};

pub mod task;
pub mod task_queue;
pub mod tuktuk_config;

/// Common options for commands
#[derive(Debug, Args, Clone)]
pub struct Opts {
    /// Anchor wallet keypair
    #[arg(short = 'w', long)]
    wallet: Option<PathBuf>,

    /// Solana RPC URL to use.
    #[arg(long, short)]
    url: String,
}

impl Opts {
    pub fn default_wallet_path() -> PathBuf {
        let mut path = home_dir().unwrap_or_else(|| PathBuf::from("/"));
        path.push(".config/solana/id.json");
        path
    }

    pub fn load_solana_keypair(&self) -> Result<Keypair> {
        let path = self
            .wallet
            .as_ref()
            .cloned()
            .unwrap_or_else(Opts::default_wallet_path);
        Keypair::read_from_file(path).map_err(|_| anyhow::anyhow!("Failed to read keypair"))
    }

    pub fn ws_url(&self) -> String {
        self.url
            .replace("https", "wss")
            .replace("http", "ws")
            .replace("127.0.0.1:8899", "127.0.0.1:8900")
    }

    pub fn rpc_url(&self) -> String {
        self.url.clone()
    }

    pub async fn client(&self) -> Result<CliClient> {
        CliClient::new(self).await
    }
}
