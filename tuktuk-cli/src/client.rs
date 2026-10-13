use std::sync::Arc;

use solana_client::{
    nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
    rpc_config::RpcSendTransactionConfig,
    send_and_confirm_transactions_in_parallel::{
        send_and_confirm_transactions_in_parallel_v2, SendAndConfirmConfigV2,
    },
    tpu_client::{TpuClientConfig, TpuSenderError},
};
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, message::Message,
    signature::Keypair, signer::Signer,
};
use solana_transaction_utils::{
    pack::pack_instructions_into_transactions, priority_fee::auto_compute_limit_and_price,
};

use crate::{cmd::Opts, result::Result};

pub struct CliClient {
    pub rpc_client: Arc<RpcClient>,
    pub payer: Keypair,
    pub opts: Opts,
}

impl AsRef<RpcClient> for CliClient {
    fn as_ref(&self) -> &RpcClient {
        &self.rpc_client
    }
}

impl CliClient {
    pub async fn new(opts: &Opts) -> Result<Self> {
        let rpc_client =
            RpcClient::new_with_commitment(opts.rpc_url(), CommitmentConfig::confirmed());
        let payer = opts.load_solana_keypair()?;
        Ok(Self {
            rpc_client: Arc::new(rpc_client),
            payer,
            opts: opts.clone(),
        })
    }
}

/// Times the sender signs a transaction. Each time after the first follows an expired blockhash.
const SIGNING_COUNT: usize = 5;
/// The sender signs one time, so one signature at most can land.
const SINGLE_SIGNING_COUNT: usize = 1;

pub async fn send_instructions(
    rpc_client: Arc<RpcClient>,
    payer: &Keypair,
    ws_url: &str,
    ixs: &[Instruction],
    extra_signers: &[Keypair],
) -> Result<()> {
    send_instructions_signed(rpc_client, payer, ws_url, ixs, extra_signers, SIGNING_COUNT).await
}

/// Sends instructions that must not land more than one time. An expired blockhash gives an
/// error and no second signature.
pub async fn send_instructions_signed_once(
    rpc_client: Arc<RpcClient>,
    payer: &Keypair,
    ws_url: &str,
    ixs: &[Instruction],
    extra_signers: &[Keypair],
) -> Result<()> {
    send_instructions_signed(
        rpc_client,
        payer,
        ws_url,
        ixs,
        extra_signers,
        SINGLE_SIGNING_COUNT,
    )
    .await
    .map_err(name_unconfirmed_send)
}

/// A sender error leaves the result of the transaction unknown. A transaction error does not.
fn name_unconfirmed_send(err: anyhow::Error) -> anyhow::Error {
    if err.downcast_ref::<TpuSenderError>().is_some() {
        return err.context(
            "The transaction is not confirmed. It can land later. Read the balance before a second run.",
        );
    }
    err
}

async fn send_instructions_signed(
    rpc_client: Arc<RpcClient>,
    payer: &Keypair,
    ws_url: &str,
    ixs: &[Instruction],
    extra_signers: &[Keypair],
    signing_count: usize,
) -> Result<()> {
    let (blockhash, _) = rpc_client
        .as_ref()
        .get_latest_blockhash_with_commitment(CommitmentConfig::finalized())
        .await
        .expect("Failed to get latest blockhash");
    let txs = pack_instructions_into_transactions(&[ixs], None)?;
    let mut with_auto_compute: Vec<Message> = Vec::new();
    let keys: Vec<&dyn Signer> = std::iter::once(&payer as &dyn Signer)
        .chain(extra_signers.iter().map(|k| k as &dyn Signer))
        .collect();
    for tx in txs {
        // This is just a tx with compute ixs. Skip it
        if tx.is_empty() {
            continue;
        }

        let (computed, _) = auto_compute_limit_and_price(
            &rpc_client,
            &tx.instructions,
            1.2,
            &payer.pubkey(),
            Some(blockhash),
            None,
        )
        .await
        .unwrap();
        with_auto_compute.push(Message::new(&computed, Some(&payer.pubkey())));
    }
    if with_auto_compute.is_empty() {
        return Ok(());
    }

    let tpu_client = TpuClient::new(
        "tuktuk-cli",
        rpc_client.clone(),
        ws_url,
        TpuClientConfig::default(),
    )
    .await?;

    let results = send_and_confirm_transactions_in_parallel_v2(
        rpc_client.clone(),
        Some(tpu_client),
        &with_auto_compute,
        &keys,
        SendAndConfirmConfigV2 {
            with_spinner: true,
            resign_txs_count: Some(signing_count),
            rpc_send_transaction_config: RpcSendTransactionConfig {
                skip_preflight: true,
                ..Default::default()
            },
        },
    )
    .await?;

    if let Some(err) = results.into_iter().flatten().next() {
        return Err(anyhow::Error::from(err));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use solana_client::tpu_client::TpuSenderError;
    use solana_sdk::transaction::TransactionError;

    use super::name_unconfirmed_send;

    #[test]
    fn a_send_that_ends_unconfirmed_tells_the_operator_to_read_the_balance() {
        let unconfirmed =
            anyhow::Error::from(TpuSenderError::Custom("Max retries exceeded".into()));

        let named = name_unconfirmed_send(unconfirmed);

        assert!(named.to_string().contains("Read the balance"), "{named}");
        assert!(
            format!("{named:#}").contains("Max retries exceeded"),
            "{named:#}"
        );
    }

    #[test]
    fn a_transaction_that_failed_on_chain_keeps_its_error() {
        let failed = anyhow::Error::from(TransactionError::AccountNotFound);

        let named = name_unconfirmed_send(failed);

        assert_eq!(
            named.to_string(),
            TransactionError::AccountNotFound.to_string()
        );
    }
}
