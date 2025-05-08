use solana_sdk::{message::CompileError, transaction::TransactionError};

use crate::tpu_conduit;

#[derive(Debug, thiserror::Error, Clone)]
pub enum Error {
    #[error("RPC error: {0}")]
    RpcError(String),
    #[error("Instruction error: {0}")]
    InstructionError(#[from] solana_sdk::instruction::InstructionError),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Compile error: {0}")]
    CompileError(#[from] CompileError),
    #[error("Signer error: {0}")]
    SignerError(String),
    #[error("Ix group too large")]
    IxGroupTooLarge,
    #[error(transparent)]
    TransportError(#[from] tpu_conduit::Error),
    #[error("Max retries exceeded")]
    MaxRetriesExceeded,
    #[error("Transaction error: {0}")]
    TransactionError(TransactionError),
    #[error("Simulated transaction error: {0}")]
    SimulatedTransactionError(TransactionError),
    #[error("Raw simulated transaction error: {0}")]
    RawSimulatedTransactionError(String),
    #[error("Raw transaction error: {0}")]
    RawTransactionError(String),
    #[error("Fee too high")]
    FeeTooHigh,
    #[error("Transaction has failed too many retries and gone stale")]
    StaleTransaction,
    #[error("message channel closed")]
    ChannelClosed,
}

impl From<solana_client::client_error::ClientError> for Error {
    fn from(value: solana_client::client_error::ClientError) -> Self {
        Self::RpcError(value.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(_value: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Self::ChannelClosed
    }
}

impl Error {
    pub fn signer<S: ToString>(str: S) -> Self {
        Self::SignerError(str.to_string())
    }

    pub fn serialization<S: ToString>(str: S) -> Self {
        Self::SerializationError(str.to_string())
    }

    pub fn channel_closed() -> Error {
        Error::ChannelClosed
    }
}
