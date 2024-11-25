#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("RPC error: {0}")]
    RpcError(#[from] solana_client::client_error::ClientError),
}
