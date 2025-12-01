use std::{collections::HashMap, sync::Arc};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table::{state::AddressLookupTable, AddressLookupTableAccount},
    pubkey::Pubkey,
};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;
use tuktuk_sdk::client::LookupTableResolver;

use crate::{cache::LookupTableRequest, sync};

pub type LookupTablesSender = sync::MessageSender<LookupTableRequest>;
pub type LookupTablesReceiver = sync::MessageReceiver<LookupTableRequest>;

impl LookupTablesSender {
    pub async fn get_lookup_tables(
        &self,
        lookup_table_keys: Vec<Pubkey>,
    ) -> Result<Vec<AddressLookupTableAccount>, sync::Error> {
        self.request(|resp| LookupTableRequest::Get {
            lookup_table_keys,
            resp,
        })
        .await
    }
}

#[async_trait::async_trait]
impl LookupTableResolver for LookupTablesSender {
    async fn resolve_lookup_tables(
        &self,
        lookup_tables: Vec<Pubkey>,
    ) -> Result<Vec<AddressLookupTableAccount>, tuktuk_sdk::error::Error> {
        self.get_lookup_tables(lookup_tables).await.map_err(|_| {
            tuktuk_sdk::error::Error::InvalidTransaction("lookup tables channel closed")
        })
    }
}

pub fn lookup_tables_channel() -> (LookupTablesSender, LookupTablesReceiver) {
    let (tx, rx) = sync::message_channel(100);
    (tx, rx)
}

pub struct LookupTablesCache {
    rpc_client: Arc<RpcClient>,
    cache: HashMap<Pubkey, AddressLookupTableAccount>,
    receiver: LookupTablesReceiver,
}

impl LookupTablesCache {
    pub fn new(rpc_client: Arc<RpcClient>, receiver: LookupTablesReceiver) -> Self {
        Self {
            rpc_client,
            cache: HashMap::new(),
            receiver,
        }
    }

    pub async fn run(mut self, handle: SubsystemHandle) -> anyhow::Result<()> {
        info!("starting lookup tables cache");
        loop {
            tokio::select! {
                _ = handle.on_shutdown_requested() => {
                    info!("shutting down lookup tables cache");
                    break;
                }
                Some(req) = self.receiver.recv() => {
                    match req {
                        LookupTableRequest::Get {
                            lookup_table_keys,
                            resp,
                        } => {
                            let mut result = Vec::with_capacity(lookup_table_keys.len());
                            let mut missing_keys = Vec::new();

                            // First check cache
                            for key in &lookup_table_keys {
                                if let Some(table) = self.cache.get(key) {
                                    result.push(table.clone());
                                } else {
                                    missing_keys.push(*key);
                                }
                            }

                            // Fetch missing tables
                            if !missing_keys.is_empty() {
                                for key in missing_keys {
                                    if let Ok(account) = self.rpc_client.get_account(&key).await {
                                        if let Ok(lut) = AddressLookupTable::deserialize(&account.data) {
                                            let table = AddressLookupTableAccount {
                                                key,
                                                addresses: lut.addresses.to_vec(),
                                            };
                                            self.cache.insert(key, table.clone());
                                            result.push(table);
                                        }
                                    }
                                }
                            }

                            // Sort result to match input order
                            result.sort_by_key(|table| {
                                lookup_table_keys.iter()
                                    .position(|key| key == &table.key)
                                    .unwrap_or(usize::MAX)
                            });

                            resp.send(result);
                        }
                    }
                }
                else => break,
            }
        }
        Ok(())
    }
}
