use std::sync::Arc;

use anchor_lang::pubkey;
use futures::TryStreamExt;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::watch::Sender;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{error, info};

use crate::{error::Error, watcher::PubsubTracker};

pub const SYSVAR_CLOCK: Pubkey = pubkey!("SysvarC1ock11111111111111111111111111111111");

pub async fn track(
    now_tx: Sender<u64>,
    pubsub_tracker: Arc<PubsubTracker>,
    handle: SubsystemHandle,
) -> Result<(), Error> {
    let (clock_str, unsub) = match pubsub_tracker.watch_pubkey(SYSVAR_CLOCK).await {
        Ok((clock_str, unsub)) => (clock_str, unsub),
        Err(err) => {
            error!(?err, "Error watching clock");
            return Err(err);
        }
    };

    let stream_fut = clock_str.try_for_each({
        let now_tx = now_tx.clone();
        move |(acc, _update_type)| {
            let now_tx = now_tx.clone();
            async move {
                match bincode::deserialize::<solana_sdk::clock::Clock>(&acc.data) {
                    Ok(c) => {
                        if now_tx.send(c.unix_timestamp as u64).is_err() {
                            error!("Failed to send clock update - receiver dropped");
                            return Err(Error::ClockSendError);
                        }

                        Ok(())
                    }
                    Err(e) => {
                        error!(?e, "Failed to deserialize clock data");
                        Err(Error::ParseBincodeError(e))
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = handle.on_shutdown_requested() => {
            info!("Shutdown requested, stopping clock tracker");
            unsub().await;
            Ok(())
        }
        result = stream_fut => {
            info!("Clock stream ended");
            result
        }
    }
}
