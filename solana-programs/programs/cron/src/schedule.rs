use std::str::FromStr;

use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::Instruction,
        sysvar::instructions::{
            load_current_index_checked, load_instruction_at_checked, ID as IX_ID,
        },
    },
    system_program, InstructionData,
};
use chrono::DateTime;
use clockwork_cron::Schedule;
use tuktuk_program::{compile_transaction, CompiledTransactionV0};

use crate::{error::ErrorCode, state::CronJobV0};

/// Queue tasks this far before the cron job is scheduled to run, so the task queue does not
/// hold tasks for executions that are still a long time away.
pub const QUEUE_TASK_DELAY: i64 = 60 * 5;

/// A schedule run reads one `cron_job_transaction` record and queues one task per queue call, so
/// both the transaction it runs in and the heap it runs on grow with this. The heap binds first:
/// a run allocates past the 32KB an instruction gets somewhere above five records, where the
/// transaction only passes the 1232-byte packet limit above nine. Five is what both allow.
pub const MAX_TASKS_PER_QUEUE_CALL: u8 = 5;

/// The two halves of the seed tuktuk signs a schedule run under: its own `b"custom"` prefix for
/// the queue, then this program's prefix for the cron job.
pub const CUSTOM_SEED: &[u8] = b"custom";
pub const CRON_SEED: &[u8] = b"cron";

/// How many records one run of this cron job may take. Jobs created before the cap keep running,
/// each run taking as many records as a run can hold, so a full cycle takes more calls.
pub fn effective_tasks_per_queue_call(cron_job: &CronJobV0) -> u8 {
    cron_job
        .num_tasks_per_queue_call
        .min(MAX_TASKS_PER_QUEUE_CALL)
}

/// `tuktuk::run_task_v0`, whose account list names the task it is running. Both are pinned by
/// `tests::run_task_v0_shape` against the client this program is built with.
const RUN_TASK_V0_DISCRIMINATOR: [u8; 8] = [52, 184, 39, 129, 126, 245, 176, 237];
const RUN_TASK_V0_TASK_ACCOUNT: usize = 3;

/// The task tuktuk is running, taken from the instruction it is running under.
///
/// A cron job's schedule lives in one task at a time and the job records which. Reading the
/// running task is what lets the handler compare the two; nothing in the accounts a task is
/// handed identifies the task itself.
pub fn running_schedule_task(sysvar_instructions: &AccountInfo) -> Result<Pubkey> {
    let index = load_current_index_checked(sysvar_instructions)?;
    let ix = load_instruction_at_checked(index as usize, sysvar_instructions)?;
    require_keys_eq!(
        ix.program_id,
        tuktuk_program::tuktuk::ID,
        ErrorCode::NotRunningAsScheduledTask
    );
    require!(
        ix.data.starts_with(&RUN_TASK_V0_DISCRIMINATOR),
        ErrorCode::NotRunningAsScheduledTask
    );
    Ok(ix
        .accounts
        .get(RUN_TASK_V0_TASK_ACCOUNT)
        .ok_or(error!(ErrorCode::NotRunningAsScheduledTask))?
        .pubkey)
}

/// The first execution the schedule names strictly after `after`.
pub fn next_exec_ts(schedule: &str, after: i64) -> Result<i64> {
    let schedule = Schedule::from_str(schedule).map_err(|_| error!(ErrorCode::InvalidSchedule))?;
    let after = DateTime::from_timestamp(after, 0).ok_or(error!(ErrorCode::InvalidSchedule))?;
    schedule
        .next_after(&after)
        .map(|next| next.timestamp())
        .ok_or(error!(ErrorCode::InvalidSchedule))
}

/// tuktuk caps a task description at 40 bytes, so descriptions carry a truncated name.
pub fn trunc_name(name: &str) -> String {
    name.chars().take(32).collect()
}

/// The address tuktuk signs for while it runs this cron job's schedule task. tuktuk prefixes a
/// task's signer seeds with `["custom", <the queue the task is on>]`, so this resolves only for
/// a task on the cron job's own queue whose transaction names these seeds.
fn cron_signer(task_queue: &Pubkey, cron_job: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[CUSTOM_SEED, task_queue.as_ref(), CRON_SEED, cron_job.as_ref()],
        &tuktuk_program::tuktuk::ID,
    )
}

/// The transaction a schedule task runs: one `queue_cron_tasks_v1` naming the
/// `cron_job_transaction` records the job is up to, and declaring the signer seeds that
/// instruction requires.
pub fn compile_schedule_transaction(
    cron_job: &CronJobV0,
    cron_job_key: Pubkey,
    task_return_account_1: Pubkey,
    task_return_account_2: Pubkey,
    recorded_schedule_task: Pubkey,
) -> Result<CompiledTransactionV0> {
    let (cron_signer, bump) = cron_signer(&cron_job.task_queue, &cron_job_key);
    // The run that follows queues the records from here, and checks each one's index against
    // the same field, so the two agree on which records the task names.
    let first_transaction_id = cron_job.current_transaction_id;
    // `add_cron_transaction_v0` takes the record index from its caller, so the end of this range
    // is only as bounded as the highest index anyone has paid to create.
    let last_transaction_id =
        first_transaction_id.saturating_add(effective_tasks_per_queue_call(cron_job) as u32);
    let transactions = (first_transaction_id..last_transaction_id).map(|i| {
        AccountMeta::new_readonly(
            Pubkey::find_program_address(
                &[
                    b"cron_job_transaction",
                    cron_job_key.as_ref(),
                    &i.to_le_bytes(),
                ],
                &crate::ID,
            )
            .0,
            false,
        )
    });

    let (transaction, _) = compile_transaction(
        vec![Instruction {
            program_id: crate::ID,
            accounts: crate::__client_accounts_queue_cron_tasks_v1::QueueCronTasksV1 {
                cron_job: cron_job_key,
                task_queue: cron_job.task_queue,
                task_return_account_1,
                task_return_account_2,
                system_program: system_program::ID,
                cron_signer,
                sysvar_instructions: IX_ID,
                recorded_schedule_task,
            }
            .to_account_metas(None)
            .into_iter()
            .chain(transactions)
            .collect(),
            data: crate::instruction::QueueCronTasksV1.data(),
        }],
        vec![vec![
            CRON_SEED.to_vec(),
            cron_job_key.to_bytes().to_vec(),
            vec![bump],
        ]],
    )?;
    Ok(transaction)
}

#[cfg(test)]
mod tests {
    use anchor_lang::{Discriminator, ToAccountMetas};

    use super::*;

    use anchor_lang::solana_program::{
        instruction::{AccountMeta, Instruction},
        sysvar::instructions::{construct_instructions_data, ID as IX_ID},
    };
    use solana_instruction::{BorrowedAccountMeta, BorrowedInstruction};

    /// The instructions sysvar as the runtime would present it for one top-level instruction.
    fn sysvar_holding(ix: &Instruction) -> Vec<u8> {
        let borrowed = BorrowedInstruction {
            program_id: &ix.program_id,
            accounts: ix
                .accounts
                .iter()
                .map(|m| BorrowedAccountMeta {
                    pubkey: &m.pubkey,
                    is_signer: m.is_signer,
                    is_writable: m.is_writable,
                })
                .collect(),
            data: &ix.data,
        };
        let mut data = construct_instructions_data(&[borrowed]);
        let end = data.len() - 2;
        data[end..].copy_from_slice(&0u16.to_le_bytes());
        data
    }

    fn read_running_task(ix: &Instruction) -> Result<Pubkey> {
        let mut data = sysvar_holding(ix);
        let key = IX_ID;
        let owner = Pubkey::default();
        let mut lamports = 0u64;
        let info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );
        running_schedule_task(&info)
    }

    fn run_task_ix(program_id: Pubkey, discriminator: [u8; 8], task: Pubkey) -> Instruction {
        Instruction {
            program_id,
            accounts: (0..6)
                .map(|i| {
                    AccountMeta::new_readonly(
                        if i == RUN_TASK_V0_TASK_ACCOUNT {
                            task
                        } else {
                            Pubkey::new_unique()
                        },
                        false,
                    )
                })
                .collect(),
            data: discriminator.to_vec(),
        }
    }

    /// The running task is only meaningful when the instruction it is read out of is the one
    /// that runs tasks. Anything else puts an unrelated program's account in that position.
    #[test]
    fn running_schedule_task_reads_only_a_tuktuk_run() {
        let task = Pubkey::new_unique();
        assert_eq!(
            read_running_task(&run_task_ix(
                tuktuk_program::tuktuk::ID,
                RUN_TASK_V0_DISCRIMINATOR,
                task
            ))
            .unwrap(),
            task,
        );
        assert!(read_running_task(&run_task_ix(
            Pubkey::new_unique(),
            RUN_TASK_V0_DISCRIMINATOR,
            task
        ))
        .is_err());
        assert!(
            read_running_task(&run_task_ix(tuktuk_program::tuktuk::ID, [0u8; 8], task)).is_err()
        );
    }

    /// The two facts `running_schedule_task` reads out of another program's instruction.
    #[test]
    fn run_task_v0_shape() {
        assert_eq!(
            RUN_TASK_V0_DISCRIMINATOR,
            tuktuk_program::tuktuk::client::args::RunTaskV0::DISCRIMINATOR,
        );
        let task = Pubkey::new_unique();
        let metas = tuktuk_program::tuktuk::client::accounts::RunTaskV0 {
            crank_turner: Pubkey::new_unique(),
            rent_refund: Pubkey::new_unique(),
            task_queue: Pubkey::new_unique(),
            task,
            system_program: Pubkey::new_unique(),
            sysvar_instructions: Pubkey::new_unique(),
        }
        .to_account_metas(None);
        assert_eq!(
            metas.iter().position(|m| m.pubkey == task),
            Some(RUN_TASK_V0_TASK_ACCOUNT),
        );
    }
}
