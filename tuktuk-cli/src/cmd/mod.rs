use std::path::PathBuf;

use clap::Args;
use dirs::home_dir;
use serde::Serialize;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::EncodableKey};
use tuktuk_program::{CompiledTransactionV0, TransactionSourceV0};

use crate::{client::CliClient, result::Result, serde::serde_pubkey};

pub mod cron;
pub mod cron_transaction;
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

#[derive(Serialize)]
pub enum TransactionSource {
    RemoteV0 {
        url: String,
        #[serde(with = "serde_pubkey")]
        signer: Pubkey,
    },
    CompiledV0 {
        transaction: CompiledTransaction,
    },
}

#[derive(Serialize)]
pub struct CompiledTransaction {
    pub instructions: Vec<Instruction>,
}

#[derive(Serialize)]
pub struct Instruction {
    pub data: Vec<u8>,
    #[serde(with = "serde_pubkey")]
    pub program_id: Pubkey,
    pub accounts: Vec<Account>,
}

#[derive(Serialize)]
pub struct Account {
    #[serde(with = "serde_pubkey")]
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

impl From<TransactionSourceV0> for TransactionSource {
    fn from(source: TransactionSourceV0) -> Self {
        match source {
            TransactionSourceV0::RemoteV0 { url, signer } => Self::RemoteV0 { url, signer },
            TransactionSourceV0::CompiledV0(transaction) => Self::CompiledV0 {
                transaction: transaction.into(),
            },
        }
    }
}

impl From<CompiledTransactionV0> for CompiledTransaction {
    fn from(transaction: CompiledTransactionV0) -> Self {
        Self {
            instructions: transaction
                .instructions
                .into_iter()
                .map(|i| Instruction {
                    data: i.data,
                    program_id: transaction.accounts[i.program_id_index as usize],
                    accounts: i
                        .accounts
                        .into_iter()
                        .map(|index| Account {
                            pubkey: transaction.accounts[index as usize],
                            is_signer: index
                                < transaction.num_ro_signers + transaction.num_rw_signers,
                            is_writable: index < transaction.num_rw_signers
                                || (index
                                    >= (transaction.num_rw_signers + transaction.num_ro_signers)
                                    && index
                                        < (transaction.num_rw_signers
                                            + transaction.num_ro_signers
                                            + transaction.num_rw)),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}
