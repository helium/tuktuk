use std::sync::Arc;

use solana_client::{
    nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
    send_and_confirm_transactions_in_parallel::{
        send_and_confirm_transactions_in_parallel, SendAndConfirmConfig,
    },
    tpu_client::TpuClientConfig,
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

pub async fn send_instructions(
    rpc_client: Arc<RpcClient>,
    payer: &Keypair,
    ws_url: &str,
    ixs: Vec<Instruction>,
    extra_signers: &[Keypair],
) -> Result<()> {
    let (blockhash, _) = rpc_client
        .as_ref()
        .get_latest_blockhash_with_commitment(CommitmentConfig::finalized())
        .await
        .expect("Failed to get latest blockhash");
    let txs = pack_instructions_into_transactions(vec![ixs], payer, None)?;
    let mut with_auto_compute: Vec<Message> = Vec::new();
    let keys: Vec<&dyn Signer> = std::iter::once(&payer as &dyn Signer)
        .chain(extra_signers.iter().map(|k| k as &dyn Signer))
        .collect();
    for (tx, _) in &txs {
        // This is just a tx with compute ixs. Skip it
        if tx.len() == 2 {
            continue;
        }

        let (computed, _) = auto_compute_limit_and_price(
            &rpc_client,
            tx.clone(),
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

    let results = send_and_confirm_transactions_in_parallel(
        rpc_client.clone(),
        Some(tpu_client),
        &with_auto_compute,
        &keys,
        SendAndConfirmConfig {
            with_spinner: true,
            resign_txs_count: Some(5),
        },
    )
    .await?;

    if let Some(err) = results.into_iter().flatten().next() {
        return Err(anyhow::Error::from(err));
    }

    Ok(())
}
