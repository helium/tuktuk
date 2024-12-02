use std::result::Result;

use anchor_lang::{prelude::AccountMeta, InstructionData, ToAccountMetas};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use spl_associated_token_account::get_associated_token_address;
use tuktuk_program::{tuktuk, TaskV0, TuktukConfigV0};

use crate::{client::GetAnchorAccount, error::Error, tuktuk::config_key};

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
            program_id: crate::program::ID,
            accounts: all_accounts,
            data: tuktuk::client::args::RunTaskV0 {}.data(),
        }))
    } else {
        Ok(None)
    }
}
