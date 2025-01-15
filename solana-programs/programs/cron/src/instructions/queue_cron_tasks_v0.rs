use std::str::FromStr;

use anchor_lang::{
    prelude::*,
    solana_program::{instruction::Instruction, program::MAX_RETURN_DATA},
    InstructionData,
};
use chrono::{DateTime, Utc};
use clockwork_cron::Schedule;
use tuktuk_program::{
    compile_transaction, RunTaskReturnV0, TaskQueueV0, TaskReturnV0, TransactionSourceV0, TriggerV0,
};

use crate::{
    error::ErrorCode,
    state::{CronJobTransactionV0, CronJobV0},
};

pub const QUEUED_TASKS_PER_QUEUE: u8 = 3;
// Queue tasks 5 minutes before the cron job is scheduled to run
// This way we don't take up space in the task queue for tasks that are running
// A long time from now
pub const QUEUE_TASK_DELAY: i64 = 60 * 5;

#[derive(Accounts)]
pub struct QueueCronTasksV0<'info> {
    #[account(
        mut,
        has_one = task_queue
    )]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
}

pub fn handler(ctx: Context<QueueCronTasksV0>) -> Result<RunTaskReturnV0> {
    let mut tasks = vec![];

    // Add as man
    let mut i = 0;
    while ctx.accounts.cron_job.current_transaction_id < ctx.accounts.cron_job.next_transaction_id
        && i < QUEUED_TASKS_PER_QUEUE as usize
    {
        let transaction = ctx.remaining_accounts[i].clone();
        if !transaction.data_is_empty() {
            let parsed_transaction: CronJobTransactionV0 =
                AccountDeserialize::try_deserialize(&mut &transaction.data.borrow()[..])?;
            let new_task = TaskReturnV0 {
                trigger: TriggerV0::Timestamp(ctx.accounts.cron_job.current_exec_ts),
                transaction: parsed_transaction.transaction,
                crank_reward: None,
                free_tasks: ctx.accounts.cron_job.free_tasks_per_transaction,
            };

            // Calculate size with new task
            let return_value = RunTaskReturnV0 {
                tasks: tasks
                    .iter()
                    .chain(std::iter::once(&new_task))
                    .cloned()
                    .collect(),
            };
            let new_size = return_value.try_to_vec()?.len();

            // Leave some room for the next re-queue
            if new_size > (MAX_RETURN_DATA - ((QUEUED_TASKS_PER_QUEUE as usize) * 32 + 128)) {
                break;
            }

            tasks.push(new_task);
        }

        ctx.accounts.cron_job.current_transaction_id += 1;
        i += 1;
    }

    // If we reached the end this time, reset to 0 and move the next execution time forward
    if ctx.accounts.cron_job.current_transaction_id == ctx.accounts.cron_job.next_transaction_id {
        ctx.accounts.cron_job.current_transaction_id = 0;
        let schedule = Schedule::from_str(&ctx.accounts.cron_job.schedule).unwrap();
        // Find the next execution time after the last one
        let ts = ctx.accounts.cron_job.current_exec_ts;
        let date_time_ts = &DateTime::<Utc>::from_naive_utc_and_offset(
            DateTime::from_timestamp(ts, 0).unwrap().naive_utc(),
            Utc,
        );
        ctx.accounts.cron_job.current_exec_ts =
            schedule.next_after(date_time_ts).unwrap().timestamp();
        msg!(
            "Finished execution ts: {}, moving to {}",
            ts,
            ctx.accounts.cron_job.current_exec_ts
        );
    }

    let remaining_accounts = (ctx.accounts.cron_job.current_transaction_id
        ..ctx.accounts.cron_job.current_transaction_id + (QUEUED_TASKS_PER_QUEUE - 1) as u32)
        .map(|i| {
            Pubkey::find_program_address(
                &[
                    b"cron_job_transaction",
                    ctx.accounts.cron_job.key().as_ref(),
                    &i.to_le_bytes(),
                ],
                &crate::ID,
            )
            .0
        })
        .collect::<Vec<Pubkey>>();

    let (queue_tx, _) = compile_transaction(
        vec![Instruction {
            program_id: crate::ID,
            accounts: [
                crate::__cpi_client_accounts_queue_cron_tasks_v0::QueueCronTasksV0 {
                    cron_job: ctx.accounts.cron_job.to_account_info(),
                    task_queue: ctx.accounts.task_queue.to_account_info(),
                }
                .to_account_metas(None),
                remaining_accounts
                    .iter()
                    .map(|pubkey| AccountMeta::new_readonly(*pubkey, false))
                    .collect::<Vec<AccountMeta>>(),
            ]
            .concat(),
            data: crate::instruction::QueueCronTasksV0.data(),
        }],
        vec![],
    )?;

    tasks.push(TaskReturnV0 {
        trigger: TriggerV0::Timestamp(ctx.accounts.cron_job.current_exec_ts - QUEUE_TASK_DELAY),
        transaction: TransactionSourceV0::CompiledV0(queue_tx),
        crank_reward: None,
        free_tasks: QUEUED_TASKS_PER_QUEUE,
    });

    // Transfer needed lamports from the cron job to the task queue
    let cron_job_info = ctx.accounts.cron_job.to_account_info();
    let cron_job_min_lamports = Rent::get()?.minimum_balance(cron_job_info.data_len());
    let lamports = ctx.accounts.task_queue.min_crank_reward * tasks.len() as u64;
    require_gt!(
        cron_job_info.lamports(),
        cron_job_min_lamports + lamports,
        ErrorCode::InsufficientFunds
    );

    **cron_job_info.try_borrow_mut_lamports()? -= lamports;
    **ctx
        .accounts
        .task_queue
        .to_account_info()
        .try_borrow_mut_lamports()? += lamports;

    msg!("Queuing {} tasks", tasks.len());
    Ok(RunTaskReturnV0 { tasks })
}
