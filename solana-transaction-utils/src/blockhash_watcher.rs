use crate::Error;
use futures::{future, TryFutureExt};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::hash::Hash;
use std::{sync::Arc, time::Duration};
use tokio::{sync::watch, time};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{info, warn};

pub type MessageSender = watch::Sender<BlockHashData>;
pub type MessageReceiver = watch::Receiver<BlockHashData>;
pub const BLOCKHASH_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

pub fn last_valid<T>(receiver: &watch::Receiver<T>) -> watch::Ref<'_, T>
where
    T: Clone,
{
    receiver.borrow()
}

#[derive(Debug, Clone, Default)]
pub struct BlockHashData {
    pub last_valid_block_height: u64,
    pub last_valid_blockhash: Hash,
    pub current_block_height: u64,
}

#[derive(Clone)]
pub struct BlockhashWatcher {
    watch: MessageSender,
    interval: Duration,
    client: Arc<RpcClient>,
}

impl BlockhashWatcher {
    pub fn new(interval: Duration, client: Arc<RpcClient>) -> Self {
        let (watch, _) = watch::channel(Default::default());
        Self {
            watch,
            interval,
            client,
        }
    }

    pub fn watcher(&mut self) -> MessageReceiver {
        self.watch.subscribe()
    }

    pub async fn run(mut self, shutdown: SubsystemHandle) -> Result<(), Error> {
        info!("starting");
        let mut interval = time::interval(self.interval);
        loop {
            tokio::select! {
                _ = shutdown.on_shutdown_requested() => {
                    info!("shutting down");
                    return Ok(());
                }
                _ = interval.tick() => {
                        match self.fetch_data(&shutdown).await {
                            Ok(Some(new_data)) => {
                                let _ = self.watch.send_replace(new_data);
                            }
                            Ok(None) => (),
                            Err(err) => warn!(?err, "failed to get block hash data"),
                        };
                }
            }
        }
    }

    pub async fn fetch_data(
        &mut self,
        shutdown: &SubsystemHandle,
    ) -> Result<Option<BlockHashData>, Error> {
        let fetch_fut = future::try_join(
            self.client
                .get_latest_blockhash_with_commitment(self.client.commitment()),
            self.client.get_block_height(),
        )
        .map_err(Error::from)
        .map_ok(
            |((last_valid_blockhash, last_valid_block_height), current_block_height)| {
                BlockHashData {
                    last_valid_block_height,
                    last_valid_blockhash,
                    current_block_height,
                }
            },
        );
        tokio::select! {
            result = fetch_fut => result.map(Some),
            _ = shutdown.on_shutdown_requested() => Ok(None)
        }
    }
}
