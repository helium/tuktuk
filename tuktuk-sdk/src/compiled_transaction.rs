use std::{collections::HashSet, result::Result};

use anchor_lang::{prelude::AccountMeta, InstructionData, ToAccountMetas};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use tuktuk_program::{tuktuk, TaskQueueV0, TaskV0};

use crate::{client::GetAnchorAccount, error::Error};

fn next_available_task_ids_excluding_in_progress(
    task_bitmap: &[u8],
    n: u8,
    in_progress_task_ids: &HashSet<u16>,
) -> Vec<u16> {
    if n == 0 {
        return vec![];
    }

    let mut available_task_ids = Vec::new();
    for (byte_idx, byte) in task_bitmap.iter().enumerate() {
        if *byte != 0xff {
            // If byte is not all 1s
            for bit_idx in 0..8 {
                if byte & (1 << bit_idx) == 0 {
                    let id = (byte_idx * 8 + bit_idx) as u16;
                    if !in_progress_task_ids.contains(&id) {
                        available_task_ids.push(id);
                        if available_task_ids.len() == n as usize {
                            return available_task_ids;
                        }
                    }
                }
            }
        }
    }
    available_task_ids
}

pub struct RunTaskResult {
    pub instruction: Instruction,
    pub free_task_ids: Vec<u16>,
}

pub async fn run_ix(
    client: &impl GetAnchorAccount,
    task_key: Pubkey,
    payer: Pubkey,
    in_progress_task_ids: &HashSet<u16>,
) -> Result<Option<RunTaskResult>, Error> {
    let task: TaskV0 = client
        .anchor_account(&task_key)
        .await?
        .ok_or_else(|| Error::AccountNotFound)?;

    let task_queue: TaskQueueV0 = client
        .anchor_account(&task.task_queue)
        .await?
        .ok_or_else(|| Error::AccountNotFound)?;

    // Get next available task IDs excluding in-progress ones
    let next_available = next_available_task_ids_excluding_in_progress(
        &task_queue.task_bitmap,
        task.free_tasks,
        in_progress_task_ids,
    );

    let transaction = &task.transaction;

    let remaining_accounts: Vec<AccountMeta> = task
        .transaction
        .accounts
        .iter()
        .enumerate()
        .map(|(index, acc)| {
            let is_writable = index < transaction.num_rw_signers as usize
                || (index >= (transaction.num_rw_signers + transaction.num_ro_signers) as usize
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

    let free_tasks = next_available
        .iter()
        .map(|id| AccountMeta {
            pubkey: Pubkey::find_program_address(
                &[b"task", task.task_queue.as_ref(), &id.to_le_bytes()],
                &tuktuk_program::ID,
            )
            .0,
            is_signer: false,
            is_writable: true,
        })
        .collect::<Vec<_>>();

    let ix_accounts = tuktuk_program::client::accounts::RunTaskV0 {
        rent_refund: task.rent_refund,
        task_queue: task.task_queue,
        task: task_key,
        crank_turner: payer,
        system_program: solana_sdk::system_program::id(),
    };

    let all_accounts = [
        ix_accounts.to_account_metas(None),
        remaining_accounts,
        free_tasks,
    ]
    .concat();

    Ok(Some(RunTaskResult {
        instruction: Instruction {
            program_id: tuktuk_program::ID,
            accounts: all_accounts,
            data: tuktuk::client::args::RunTaskV0 {
                args: tuktuk_program::types::RunTaskArgsV0 {
                    free_task_ids: next_available.clone(),
                },
            }
            .data(),
        },
        free_task_ids: next_available,
    }))
}
