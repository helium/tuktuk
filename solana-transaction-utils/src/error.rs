use solana_sdk::{message::CompileError, signer::SignerError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("RPC error: {0}")]
    RpcError(Box<solana_client::client_error::ClientError>),
    #[error("Instruction error: {0}")]
    InstructionError(#[from] solana_sdk::instruction::InstructionError),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] bincode::Error),
    #[error("Compile error: {0}")]
    CompileError(#[from] CompileError),
    #[error("Signer error: {0}")]
    SignerError(#[from] SignerError),
    #[error("Ix group too large")]
    IxGroupTooLarge,
}

impl From<solana_client::client_error::ClientError> for Error {
    fn from(value: solana_client::client_error::ClientError) -> Self {
        Self::RpcError(Box::new(value))
    }
}
