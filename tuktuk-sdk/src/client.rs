use std::{marker::Send, sync::Arc};

use anchor_lang::AccountDeserialize;
use futures::{stream, StreamExt, TryFutureExt, TryStreamExt};
use itertools::Itertools;
pub use solana_client::nonblocking::rpc_client::RpcClient as SolanaRpcClient;
use solana_sdk::{
    account::Account, address_lookup_table::state::AddressLookupTable,
    message::AddressLookupTableAccount, pubkey::Pubkey,
};

use crate::error::Error;

#[derive(Clone)]
pub struct Client {
    pub solana_client: Arc<SolanaRpcClient>,
}

#[async_trait::async_trait]
pub trait GetAccount {
    async fn account(&self, pubkey: &Pubkey) -> Result<Option<Account>, Error>;
    async fn accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<(Pubkey, Option<Account>)>, Error>;
}

#[async_trait::async_trait]
pub trait GetAnchorAccount: GetAccount {
    async fn anchor_account<T: AccountDeserialize>(
        &self,
        pubkey: &Pubkey,
    ) -> Result<Option<T>, Error>;
    async fn anchor_accounts<T: AccountDeserialize + Send>(
        &self,
        pubkeys: &[Pubkey],
    ) -> Result<Vec<(Pubkey, Option<T>)>, Error>;
}

#[async_trait::async_trait]
impl GetAccount for SolanaRpcClient {
    async fn account(&self, pubkey: &Pubkey) -> Result<Option<Account>, Error> {
        self.get_account_with_commitment(pubkey, self.commitment())
            .map_ok(|response| response.value)
            .map_err(Error::from)
            .await
    }
    async fn accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<(Pubkey, Option<Account>)>, Error> {
        async fn get_accounts(
            client: &SolanaRpcClient,
            pubkeys: &[Pubkey],
        ) -> Result<Vec<(Pubkey, Option<Account>)>, Error> {
            let accounts = client.get_multiple_accounts(pubkeys).await?;
            Ok(pubkeys
                .iter()
                .cloned()
                .zip(accounts.into_iter())
                .collect_vec())
        }

        stream::iter(pubkeys.to_vec())
            .chunks(100)
            .map(|key_chunk| async move { get_accounts(self, &key_chunk).await })
            .buffered(5)
            .try_concat()
            .await
    }
}

#[async_trait::async_trait]
pub trait LookupTableResolver {
    async fn resolve_lookup_tables(
        &self,
        lookup_tables: Vec<Pubkey>,
    ) -> Result<Vec<AddressLookupTableAccount>, Error>;
}

#[async_trait::async_trait]
impl LookupTableResolver for SolanaRpcClient {
    async fn resolve_lookup_tables(
        &self,
        lookup_tables: Vec<Pubkey>,
    ) -> Result<Vec<AddressLookupTableAccount>, Error> {
        let accounts = self.get_multiple_accounts(&lookup_tables).await?;
        Ok(accounts
            .into_iter()
            .zip(lookup_tables.iter())
            .filter_map(|(maybe_acc, pubkey)| {
                maybe_acc.map(|acc| {
                    let lut = AddressLookupTable::deserialize(&acc.data).map_err(Error::from)?;
                    Ok(AddressLookupTableAccount {
                        key: *pubkey,
                        addresses: lut.addresses.to_vec(),
                    })
                })
            })
            .collect::<Result<Vec<_>, Error>>()?)
    }
}

#[async_trait::async_trait]
impl GetAnchorAccount for SolanaRpcClient {
    async fn anchor_account<T: AccountDeserialize>(
        &self,
        pubkey: &Pubkey,
    ) -> Result<Option<T>, Error> {
        self.account(pubkey)
            .and_then(|maybe_account| async move {
                maybe_account
                    .map(|account| {
                        T::try_deserialize(&mut account.data.as_ref()).map_err(Error::from)
                    })
                    .transpose()
            })
            .await
    }

    async fn anchor_accounts<T: AccountDeserialize + Send>(
        &self,
        pubkeys: &[Pubkey],
    ) -> Result<Vec<(Pubkey, Option<T>)>, Error> {
        self.accounts(pubkeys)
            .await?
            .into_iter()
            .map(|(pubkey, maybe_account)| {
                maybe_account
                    .map(|account| {
                        T::try_deserialize(&mut account.data.as_ref()).map_err(Error::from)
                    })
                    .transpose()
                    .map(|deser_account| (pubkey, deser_account))
            })
            .try_collect()
    }
}

#[async_trait::async_trait]
impl GetAccount for Client {
    async fn account(&self, pubkey: &Pubkey) -> Result<Option<Account>, Error> {
        self.solana_client.account(pubkey).await
    }
    async fn accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<(Pubkey, Option<Account>)>, Error> {
        self.solana_client.accounts(pubkeys).await
    }
}

#[async_trait::async_trait]
impl GetAnchorAccount for Client {
    async fn anchor_account<T: AccountDeserialize>(
        &self,
        pubkey: &Pubkey,
    ) -> Result<Option<T>, Error> {
        self.solana_client.anchor_account(pubkey).await
    }
    async fn anchor_accounts<T: AccountDeserialize + Send>(
        &self,
        pubkeys: &[Pubkey],
    ) -> Result<Vec<(Pubkey, Option<T>)>, Error> {
        self.solana_client.anchor_accounts(pubkeys).await
    }
}
