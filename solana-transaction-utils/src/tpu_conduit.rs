use futures::TryFutureExt;
use solana_client::{
    nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
    tpu_client::TpuClientConfig,
};
use solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool};
use solana_sdk::transport::TransportError;
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

pub type QuicTpuClient = TpuClient<QuicPool, QuicConnectionManager, QuicConfig>;
pub const TPU_SHUTDOWN_THRESHOLD: Duration = Duration::from_secs(60);
pub const TPU_SEND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("tpu sender: {0}")]
    TpuSenderError(String),
    #[error("tpu transport: {0}")]
    TransportError(String),
}

impl From<solana_tpu_client::tpu_client::TpuSenderError> for Error {
    fn from(value: solana_tpu_client::tpu_client::TpuSenderError) -> Self {
        Self::TpuSenderError(value.to_string())
    }
}

impl From<solana_sdk::transport::TransportError> for Error {
    fn from(value: solana_sdk::transport::TransportError) -> Self {
        Self::TransportError(value.to_string())
    }
}

pub struct TpuConduit {
    tpu_client: Option<QuicTpuClient>,
    rpc_client: Arc<RpcClient>,
    ws_url: String,
    last_send: Option<Instant>,
}

macro_rules! perform_connected {
    ($v: expr, $f: expr) => {{
        if $v.tpu_client.is_none() {
            $v.connect().await?;
        }
        match tokio::time::timeout(TPU_SEND_TIMEOUT, $f)
            .map_err(|err| TransportError::from(io::Error::from(err)))
            .await?
        {
            Ok(()) => (),
            Err(err) => {
                $v.disconnect().await;
                return Err(err.into());
            }
        }
        $v.last_send = Some(Instant::now());
        Ok(())
    }};
}

impl TpuConduit {
    pub fn new(rpc_client: Arc<RpcClient>, ws_url: impl ToString) -> Self {
        Self {
            tpu_client: None,
            rpc_client,
            ws_url: ws_url.to_string(),
            last_send: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        let tpu_client = TpuClient::new(
            "transaction-sender",
            self.rpc_client.clone(),
            &self.ws_url,
            TpuClientConfig::default(),
        )
        .await?;
        self.tpu_client = Some(tpu_client);
        self.last_send = Some(Instant::now());
        Ok(())
    }

    pub async fn disconnect(&mut self) {
        let Some(tpu_client) = self.tpu_client.as_mut() else {
            return;
        };
        tpu_client.shutdown().await;
        self.tpu_client = None;
    }

    pub async fn maybe_disconnect(&mut self) {
        if let Some(last_send) = self.last_send {
            if last_send.elapsed() > TPU_SHUTDOWN_THRESHOLD {
                self.disconnect().await;
                self.last_send = None;
            }
        }
    }

    pub async fn send_batch(&mut self, wire_txns: Vec<Vec<u8>>) -> Result<(), Error> {
        perform_connected!(
            self,
            self.tpu_client
                .as_mut()
                // safe to unwrap
                .unwrap()
                .try_send_wire_transaction_batch(wire_txns)
        )
    }

    pub async fn send(&mut self, wire_txn: Vec<u8>) -> Result<(), Error> {
        perform_connected!(
            self,
            self.tpu_client
                .as_mut()
                // safe to unwrap
                .unwrap()
                .try_send_wire_transaction(wire_txn)
        )
    }
}
