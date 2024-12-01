use std::{collections::HashMap, result::Result};

use anchor_lang::{prelude::AccountMeta, InstructionData, ToAccountMetas};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use spl_associated_token_account::get_associated_token_address;

use crate::{
    client::GetAnchorAccount,
    error::Error,
    tuktuk::{
        config_key, tuktuk,
        types::{CompiledInstructionV0, CompiledTransactionArgV0, CompiledTransactionV0},
        TaskV0, TuktukConfigV0,
    },
};

impl From<CompiledTransactionV0> for CompiledTransactionArgV0 {
    fn from(value: CompiledTransactionV0) -> Self {
        CompiledTransactionArgV0 {
            num_ro_signers: value.num_ro_signers,
            num_rw_signers: value.num_rw_signers,
            num_rw: value.num_rw,
            instructions: value.instructions,
            signer_seeds: value.signer_seeds,
        }
    }
}

pub fn compile_transaction(
    instructions: Vec<Instruction>,
    signer_seeds: Vec<Vec<Vec<u8>>>,
) -> Result<(CompiledTransactionV0, Vec<AccountMeta>), Error> {
    let mut pubkeys_to_metadata: HashMap<Pubkey, AccountMeta> = HashMap::new();

    // Process all instructions to build metadata
    for ix in &instructions {
        pubkeys_to_metadata
            .entry(ix.program_id)
            .or_insert(AccountMeta {
                pubkey: ix.program_id,
                is_signer: false,
                is_writable: false,
            });

        for key in &ix.accounts {
            let entry = pubkeys_to_metadata
                .entry(key.pubkey)
                .or_insert(AccountMeta {
                    is_signer: false,
                    is_writable: false,
                    pubkey: key.pubkey,
                });
            entry.is_writable |= key.is_writable;
            entry.is_signer |= key.is_signer;
        }
    }

    // Sort accounts: writable signers first, then ro signers, then rw non-signers, then ro
    let mut sorted_accounts: Vec<Pubkey> = pubkeys_to_metadata.keys().cloned().collect();
    sorted_accounts.sort_by(|a, b| {
        let a_meta = &pubkeys_to_metadata[a];
        let b_meta = &pubkeys_to_metadata[b];

        // Compare accounts based on priority: writable signers > readonly signers > writable > readonly
        fn get_priority(meta: &AccountMeta) -> u8 {
            match (meta.is_signer, meta.is_writable) {
                (true, true) => 0,   // Writable signer: highest priority
                (true, false) => 1,  // Readonly signer
                (false, true) => 2,  // Writable non-signer
                (false, false) => 3, // Readonly non-signer: lowest priority
            }
        }

        get_priority(a_meta).cmp(&get_priority(b_meta))
    });

    // Count different types of accounts
    let mut num_rw_signers = 0u8;
    let mut num_ro_signers = 0u8;
    let mut num_rw = 0u8;

    for k in &sorted_accounts {
        let metadata = &pubkeys_to_metadata[k];
        if metadata.is_signer && metadata.is_writable {
            num_rw_signers += 1;
        } else if metadata.is_signer && !metadata.is_writable {
            num_ro_signers += 1;
        } else if metadata.is_writable {
            num_rw += 1;
        }
    }

    // Create accounts to index mapping
    let accounts_to_index: HashMap<Pubkey, u8> = sorted_accounts
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, i as u8))
        .collect();

    // Compile instructions
    let compiled_instructions: Vec<CompiledInstructionV0> = instructions
        .iter()
        .map(|ix| CompiledInstructionV0 {
            program_id_index: *accounts_to_index.get(&ix.program_id).unwrap(),
            accounts: ix
                .accounts
                .iter()
                .map(|k| *accounts_to_index.get(&k.pubkey).unwrap())
                .collect(),
            data: ix.data.clone(),
        })
        .collect();

    let remaining_accounts = sorted_accounts
        .iter()
        .enumerate()
        .map(|(index, k)| AccountMeta {
            pubkey: *k,
            is_signer: false,
            is_writable: index < num_rw_signers as usize
                || (index >= num_rw_signers as usize + num_ro_signers as usize
                    && index < num_rw_signers as usize + num_ro_signers as usize + num_rw as usize),
        })
        .collect();

    Ok((
        CompiledTransactionV0 {
            num_ro_signers,
            num_rw_signers,
            num_rw,
            instructions: compiled_instructions,
            signer_seeds,
            accounts: sorted_accounts,
        },
        remaining_accounts,
    ))
}

pub async fn run_ix<C: GetAnchorAccount>(
    client: &C,
    task: Pubkey,
    rewards_destination_wallet: Pubkey,
) -> Result<Option<Instruction>, Error> {
    let task_account = client.anchor_account::<TaskV0>(&task).await?;
    let config_account = client
        .anchor_account::<TuktukConfigV0>(&config_key())
        .await?
        .ok_or_else(|| Error::AccountNotFound)?;

    if let Some(task_account) = task_account {
        let transaction = &task_account.transaction;

        let remaining_accounts: Vec<AccountMeta> = task_account
            .transaction
            .accounts
            .iter()
            .enumerate()
            .map(|(index, acc)| {
                let is_writable = index < transaction.num_rw_signers as usize
                    || (index
                        >= (transaction.num_rw_signers + transaction.num_ro_signers) as usize
                        && index
                            < (transaction.num_rw_signers
                                + transaction.num_ro_signers
                                + transaction.num_rw) as usize);

                AccountMeta {
                    pubkey: *acc,
                    is_signer: false,
                    is_writable,
                }
            })
            .collect();

        let ix_accounts = tuktuk::client::accounts::RunTaskV0 {
            rent_refund: task_account.rent_refund,
            task_queue: task_account.task_queue,
            task,
            rewards_destination: get_associated_token_address(
                &rewards_destination_wallet,
                &config_account.network_mint,
            ),
            rewards_source: get_associated_token_address(
                &task_account.task_queue,
                &config_account.network_mint,
            ),
            token_program: spl_token::id(),
        };
        let all_accounts = [ix_accounts.to_account_metas(None), remaining_accounts].concat();
        Ok(Some(Instruction {
            program_id: crate::tuktuk::ID,
            accounts: all_accounts,
            data: tuktuk::client::args::RunTaskV0 {}.data(),
        }))
    } else {
        Ok(None)
    }
}
