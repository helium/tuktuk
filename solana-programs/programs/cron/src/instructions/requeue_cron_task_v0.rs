use anchor_lang::prelude::*;
use tuktuk_program::{
    tuktuk::{
        cpi::{accounts::QueueTaskV0, queue_task_v0},
        program::Tuktuk,
    },
    types::QueueTaskArgsV0,
    TaskQueueAuthorityV0, TaskQueueV0, TransactionSourceV0, TriggerV0,
};

use crate::{
    error::ErrorCode,
    schedule::{compile_schedule_transaction, next_exec_ts, trunc_name, QUEUE_TASK_DELAY},
    state::CronJobV0,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RequeueCronTaskArgsV0 {
    pub task_id: u16,
}

#[derive(Accounts)]
#[instruction(args: RequeueCronTaskArgsV0)]
pub struct RequeueCronTaskV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub authority: Signer<'info>,
    pub queue_authority: Signer<'info>,
    #[account(
        seeds = [b"task_queue_authority", task_queue.key().as_ref(), queue_authority.key().as_ref()],
        bump = task_queue_authority.bump_seed,
        seeds::program = tuktuk_program.key(),
    )]
    pub task_queue_authority: Box<Account<'info, TaskQueueAuthorityV0>>,
    #[account(mut, has_one = authority)]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    /// CHECK: The task the cron job last recorded. A cron job runs one schedule chain, so a new
    /// task may only be queued once that record holds nothing. `Pubkey::default()` is the
    /// system program, which has data, so it is spelled out rather than left to the emptiness
    /// test.
    #[account(
        address = cron_job.next_schedule_task,
        constraint = cron_job.next_schedule_task == Pubkey::default()
            || next_schedule_task.data_is_empty() @ ErrorCode::TaskAlreadyQueued,
    )]
    pub next_schedule_task: UncheckedAccount<'info>,
    #[account(mut)]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    /// CHECK: Initialized in CPI
    #[account(mut)]
    pub task: AccountInfo<'info>,
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
    pub tuktuk_program: Program<'info, Tuktuk>,
}

pub fn handler(ctx: Context<RequeueCronTaskV0>, args: RequeueCronTaskArgsV0) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;

    ctx.accounts.cron_job.next_schedule_task = ctx.accounts.task.key();
    ctx.accounts.cron_job.removed_from_queue = false;
    ctx.accounts.cron_job.current_exec_ts = next_exec_ts(&ctx.accounts.cron_job.schedule, now)?;
    // The new chain runs whole executions, so it starts at the first transaction. Any count the
    // interrupted chain left behind belongs to an execution that is over.
    ctx.accounts.cron_job.current_transaction_id = 0;

    let queue_tx = compile_schedule_transaction(
        &ctx.accounts.cron_job,
        ctx.accounts.cron_job.key(),
        ctx.accounts.task_return_account_1.key(),
        ctx.accounts.task_return_account_2.key(),
        ctx.accounts.task.key(),
    )?;

    let trunc_name = trunc_name(&ctx.accounts.cron_job.name);
    queue_task_v0(
        CpiContext::new(
            ctx.accounts.tuktuk_program.to_account_info(),
            QueueTaskV0 {
                payer: ctx.accounts.payer.to_account_info(),
                queue_authority: ctx.accounts.queue_authority.to_account_info(),
                task_queue_authority: ctx.accounts.task_queue_authority.to_account_info(),
                task_queue: ctx.accounts.task_queue.to_account_info(),
                task: ctx.accounts.task.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
        ),
        QueueTaskArgsV0 {
            trigger: TriggerV0::Timestamp(ctx.accounts.cron_job.current_exec_ts - QUEUE_TASK_DELAY),
            transaction: TransactionSourceV0::CompiledV0(queue_tx),
            crank_reward: None,
            free_tasks: ctx.accounts.cron_job.num_tasks_per_queue_call + 1,
            id: args.task_id,
            description: format!("queue {}", trunc_name),
        },
    )?;

    Ok(())
}
