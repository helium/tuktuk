use solana_client::{client_error::reqwest, pubsub_client::PubsubClientError};
use solana_sdk::{program_error::ProgramError, pubkey::ParsePubkeyError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("RPC error: {0}")]
    RpcError(Box<solana_client::client_error::ClientError>),
    #[error("Failed to parse bincode: {0}")]
    ParseBincodeError(#[from] Box<bincode::ErrorKind>),
    #[error("Anchor error: {0}")]
    AnchorError(#[from] anchor_lang::error::Error),
    #[error("Solana Pubsub error: {0}")]
    SolanaPubsubError(#[from] PubsubClientError),
    #[error("Program error: {0}")]
    ProgramError(#[from] ProgramError),
    #[error("Account required for the instruction was not found")]
    AccountNotFound,
    #[error("Invalid prepayment: {slots_immediately_for_auction} slots will immediately be auctioned off, but only {num_prepaid_segment_slots} prepaid slots were provided")]
    InvalidPrepayment {
        slots_immediately_for_auction: u64,
        num_prepaid_segment_slots: u64,
    },
    #[error("Too many tasks")]
    TooManyTasks,
    #[error("Price arithmetic error")]
    PriceArithmeticError,
    #[error("Failed to fetch remote transaction")]
    FetchRemoteTransactionError(#[from] reqwest::Error),
    #[error("Failed to decode base64")]
    DecodeBase64Error(#[from] base64::DecodeError),
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(&'static str),
    #[error("Failed to parse pubkey: {0}")]
    ParsePubkeyError(#[from] ParsePubkeyError),
}

impl From<solana_client::client_error::ClientError> for Error {
    fn from(value: solana_client::client_error::ClientError) -> Self {
        Self::RpcError(Box::new(value))
    }
}
