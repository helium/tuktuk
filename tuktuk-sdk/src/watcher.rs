use std::{
    collections::HashMap, pin::Pin, result::Result, str::FromStr, sync::Arc, time::Duration,
};

use futures::{
    future::{BoxFuture, Future},
    stream::{unfold, Stream, StreamExt},
};
use solana_account_decoder_client_types::{UiAccount, UiAccountEncoding};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcAccountInfoConfig;
use solana_sdk::{account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey};
use tokio::{
    sync::{broadcast, Mutex},
    time::interval,
};

use crate::{client::GetAccount, error::Error, pubsub_client::PubsubClient};

pub struct PubsubTracker {
    client: Arc<RpcClient>,
    pubsub: Arc<PubsubClient>,
    watched_pubkeys: Arc<Mutex<HashMap<Pubkey, Option<Account>>>>,
    publisher: broadcast::Sender<(Pubkey, Account)>, // Change to broadcast::Sender
    requery_interval: Duration,
    commitment: CommitmentConfig,
}

fn account_from_ui_account(value: &UiAccount) -> Account {
    Account {
        lamports: value.lamports,
        data: value.data.decode().unwrap_or_default(),
        owner: Pubkey::from_str(&value.owner).unwrap_or_default(),
        executable: value.executable,
        rent_epoch: value.rent_epoch,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpdateType {
    Poll,
    Websocket,
}

impl PubsubTracker {
    pub fn new(
        client: Arc<RpcClient>,
        pubsub: Arc<PubsubClient>,
        requery_interval: Duration,
        commitment: CommitmentConfig,
    ) -> Self {
        Self {
            client,
            pubsub,
            watched_pubkeys: Arc::new(Mutex::new(HashMap::new())),
            publisher: broadcast::Sender::new(1000),
            requery_interval,
            commitment,
        }
    }

    pub async fn watch_pubkey<'a>(
        &'a self,
        pubkey: Pubkey,
    ) -> Result<
        (
            impl Stream<Item = Result<(Account, UpdateType), Error>> + 'a,
            Box<dyn FnOnce() -> BoxFuture<'a, ()> + Send + 'a>,
        ),
        Error,
    > {
        // Subscribe to account updates
        let (subscription, unsub) = self
            .pubsub
            .account_subscribe(
                &solana_pubkey::Pubkey::from_str(&pubkey.to_string())?,
                Some(RpcAccountInfoConfig {
                    commitment: Some(self.commitment),
                    encoding: Some(UiAccountEncoding::Base64Zstd),
                    ..Default::default()
                }),
            )
            .await?;

        self.watched_pubkeys.lock().await.insert(pubkey, None);

        let publisher_receiver = self.publisher.subscribe();
        let client = self.client.clone();

        let stream = unfold(
            (subscription, publisher_receiver),
            move |(mut subscription, mut publisher_receiver)| {
                let pubkey = pubkey;
                let client = client.clone();
                async move {
                    loop {
                        tokio::select! {
                            Some(s) = subscription.next() => {
                                let mut account = account_from_ui_account(&s.value);
                                if account.data.is_empty() {
                                    account = match client.get_account_with_commitment(&pubkey, self.commitment).await {
                                        Ok(acc) => acc.value.unwrap_or(account),
                                        Err(e) => return Some((Err(Error::from(e)), (subscription, publisher_receiver))),
                                    };
                                }
                                return Some((Ok((account, UpdateType::Websocket)), (subscription, publisher_receiver)));
                            },
                            Ok((key, acc)) = publisher_receiver.recv() => {
                                if key == pubkey {
                                    return Some((Ok((acc, UpdateType::Poll)), (subscription, publisher_receiver)));
                                }
                            },
                            else => break,
                        }
                    }
                    None
                }
            },
        );

        let unsubscribe = Box::new({
            let pubkey_clone = pubkey;
            move || {
                Box::pin(async move {
                    self.watched_pubkeys.lock().await.remove(&pubkey_clone);
                    unsub().await
                }) as Pin<Box<dyn Future<Output = ()> + Send>>
            }
        });

        Ok((stream, unsubscribe))
    }

    pub async fn start_tracking(&self) {
        let mut interval = interval(self.requery_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.check_for_changes().await {
                eprintln!("Error checking for changes: {:?}", e);
            }
        }
    }

    pub async fn check_for_changes(&self) -> Result<(), Error> {
        let pubkeys: Vec<_> = self.watched_pubkeys.lock().await.keys().cloned().collect();
        let accounts = self.client.accounts(&pubkeys).await?;

        let mut watched_data = self.watched_pubkeys.lock().await;
        accounts
            .into_iter()
            .map(|(pubkey, maybe_account)| {
                if let Some(account) = maybe_account {
                    if let Some(stored_data) = watched_data.get_mut(&pubkey) {
                        if stored_data.as_ref().map(|d| *d != account).unwrap_or(true) {
                            // Publish the change to the output stream
                            self.publish_change(pubkey, account.clone());
                            *stored_data = Some(account); // Update stored data
                        }
                    }
                }

                Ok(())
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(())
    }

    fn publish_change(&self, pubkey: Pubkey, account_data: Account) {
        if self.publisher.send((pubkey, account_data.clone())).is_err() {
            eprintln!("Failed to send update for pubkey: {:?}", pubkey);
        }
    }
}
