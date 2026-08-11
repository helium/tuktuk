use anchor_lang::prelude::*;
use tuktuk_program::{RunTaskReturnV0, TaskQueueV0, TaskReturnV0, TransactionSourceV0, TriggerV0};

use crate::{
    schedule::{compile_schedule_transaction, effective_tasks_per_queue_call, trunc_name},
    state::CronJobV0,
};

#[derive(Accounts)]
pub struct QueueCronTasksV0<'info> {
    /// CHECK: Checked via require in handler
    #[account(mut)]
    pub cron_job: UncheckedAccount<'info>,
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    /// CHECK: Used to write return data
    #[account(
        mut,
        seeds = [b"task_return_account_1", cron_job.key().as_ref()],
        bump
    )]
    pub task_return_account_1: AccountInfo<'info>,
    /// CHECK: Used to write return data
    #[account(
        mut,
        seeds = [b"task_return_account_2", cron_job.key().as_ref()],
        bump
    )]
    pub task_return_account_2: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[macro_export]
macro_rules! try_from {
    ($ty: ty, $acc: expr) => {{
        let account_info = $acc.as_ref();
        <$ty>::try_from(unsafe {
            core::mem::transmute::<
                &anchor_lang::prelude::AccountInfo<'_>,
                &anchor_lang::prelude::AccountInfo<'_>,
            >(account_info)
        })
    }};
}

/// Hands the schedule over to `queue_cron_tasks_v1`, which carries the accounts this account
/// list has no room for. Reads the cron job and returns; the schedule advances, the lamports
/// move and the successor is recorded there, on a call that carries the signer.
///
/// Schedule tasks compiled before v1 existed still name this instruction, so each one converts
/// itself the next time it runs. The successor triggers on `Now`, which `run_task_v0` measures
/// as the current slot's time, so it stays runnable whatever a queue's `stale_task_age` is.
pub fn handler(ctx: Context<QueueCronTasksV0>) -> Result<RunTaskReturnV0> {
    if ctx.accounts.cron_job.data_is_empty() {
        msg!("Cron job was closed, completing task");
        return Ok(RunTaskReturnV0 {
            tasks: vec![],
            accounts: vec![],
        });
    }
    let cron_job = try_from!(Account<CronJobV0>, &ctx.accounts.cron_job)?;
    require_eq!(cron_job.task_queue, ctx.accounts.task_queue.key());

    let queue_tx = compile_schedule_transaction(
        &cron_job,
        cron_job.key(),
        ctx.accounts.task_return_account_1.key(),
        ctx.accounts.task_return_account_2.key(),
        cron_job.next_schedule_task,
    )?;

    // One task fits the return data, so no return account is written and the cron job's
    // lamports are untouched. The successor is funded by the task queue: this account list
    // carries no signer for the job, and the job's lamports only move under one. The queue
    // pays the conversion once per chain, and every run after it charges the job again.
    Ok(RunTaskReturnV0 {
        tasks: vec![TaskReturnV0 {
            trigger: TriggerV0::Now,
            transaction: TransactionSourceV0::CompiledV0(queue_tx),
            crank_reward: None,
            free_tasks: effective_tasks_per_queue_call(&cron_job) + 1,
            description: format!("queue {}", trunc_name(&cron_job.name)),
        }],
        accounts: vec![],
    })
}
