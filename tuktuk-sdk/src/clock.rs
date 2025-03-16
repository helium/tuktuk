use std::sync::Arc;

use anchor_lang::pubkey;
use futures::StreamExt;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::watch::{channel, Receiver};

use crate::{error::Error, watcher::PubsubTracker};

pub const SYSVAR_CLOCK: Pubkey = pubkey!("SysvarC1ock11111111111111111111111111111111");

pub async fn track(
    rpc_client: Arc<RpcClient>,
    pubsub_tracker: Arc<PubsubTracker>,
) -> Result<Receiver<u64>, Error> {
    let clock_acc = rpc_client.get_account(&SYSVAR_CLOCK).await?;
    let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock_acc.data)?;
    let (now_tx, now_rx) = channel(clock.unix_timestamp as u64);
    tokio::spawn(async move {
        let (clock_str, _) = pubsub_tracker.watch_pubkey(SYSVAR_CLOCK).await.unwrap();
        clock_str
            .for_each(move |acc_result| {
                let now_tx = now_tx.clone();
                async move {
                    if let Ok((acc, _update_type)) = acc_result {
                        if let Ok(c) = bincode::deserialize::<solana_sdk::clock::Clock>(&acc.data) {
                            now_tx.send(c.unix_timestamp as u64).unwrap();
                        }
                    }
                }
            })
            .await;
    });

    Ok(now_rx)
}
