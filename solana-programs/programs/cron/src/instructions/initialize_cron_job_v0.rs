use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};
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
    hash_name,
    schedule::{
        compile_schedule_transaction, effective_tasks_per_queue_call, next_exec_ts, trunc_name,
        MAX_TASKS_PER_QUEUE_CALL, QUEUE_TASK_DELAY,
    },
    state::{CronJobNameMappingV0, CronJobV0, UserCronJobsV0},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct InitializeCronJobArgsV0 {
    pub schedule: String,
    pub name: String,
    /// The number of free tasks each transaction will be executed with.
    /// This allows transactions scheduled via cron to themselves schedule more transactions.
    /// If none of your transactions need to schedule more transactions, set this to 0.
    pub free_tasks_per_transaction: u8,
    /// The number of tasks to queue per queue call.
    /// Cron job works by queueing a single task that runs at the appropriate time. This tasks job
    /// is to recursively queue all transactions in this cron. The higher you set this number, the more
    /// tasks will be queued per queue call, making the tasks execute faster/more parallelized.
    /// Setting this too high without proper lookup tables will result in the queue call being too large
    pub num_tasks_per_queue_call: u8,
}

#[derive(Accounts)]
#[instruction(args: InitializeCronJobArgsV0)]
pub struct InitializeCronJobV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub queue_authority: Signer<'info>,
    #[account(
        seeds = [b"task_queue_authority", task_queue.key().as_ref(), queue_authority.key().as_ref()],
        bump = task_queue_authority.bump_seed,
        seeds::program = tuktuk_program.key(),
    )]
    pub task_queue_authority: Box<Account<'info, TaskQueueAuthorityV0>>,
    /// CHECK: Just needed as a setting
    pub authority: Signer<'info>,
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + 60 + std::mem::size_of::<UserCronJobsV0>(),
        seeds = [b"user_cron_jobs", authority.key().as_ref()],
        bump
    )]
    pub user_cron_jobs: Box<Account<'info, UserCronJobsV0>>,
    #[account(
        init,
        payer = payer,
        space = 8 + 60 + std::mem::size_of::<CronJobV0>() + args.name.len() + args.schedule.len(),
        seeds = [b"cron_job", authority.key().as_ref(), &user_cron_jobs.next_cron_job_id.to_le_bytes()[..]],
        bump
    )]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    #[account(
        init,
        payer = payer,
        space = 8 + 60 + std::mem::size_of::<CronJobNameMappingV0>() + args.name.len(),
        seeds = [
            b"cron_job_name_mapping",
            authority.key().as_ref(),
            &hash_name(args.name.as_str())
        ],
        bump
    )]
    pub cron_job_name_mapping: Account<'info, CronJobNameMappingV0>,
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

pub fn handler(ctx: Context<InitializeCronJobV0>, args: InitializeCronJobArgsV0) -> Result<()> {
    // Leave room for numerics after
    require_gt!(
        args.num_tasks_per_queue_call,
        0,
        ErrorCode::InvalidNumTasksPerQueueCall
    );

    // Bounded by what a single schedule run can allocate and still fit its transaction; see
    // MAX_TASKS_PER_QUEUE_CALL. Existing jobs above this keep running because every run is
    // clamped to the same bound; a full cycle of their records just takes more calls.
    require_gte!(
        MAX_TASKS_PER_QUEUE_CALL,
        args.num_tasks_per_queue_call,
        ErrorCode::InvalidNumTasksPerQueueCall
    );

    let current_exec_ts = next_exec_ts(&args.schedule, Clock::get()?.unix_timestamp)?;

    ctx.accounts.user_cron_jobs.bump_seed = ctx.bumps.user_cron_jobs;
    ctx.accounts.user_cron_jobs.authority = ctx.accounts.authority.key();

    ctx.accounts.cron_job.set_inner(CronJobV0 {
        id: ctx.accounts.user_cron_jobs.next_cron_job_id,
        user_cron_jobs: ctx.accounts.user_cron_jobs.key(),
        task_queue: ctx.accounts.task_queue.key(),
        authority: ctx.accounts.authority.key(),
        free_tasks_per_transaction: args.free_tasks_per_transaction,
        num_tasks_per_queue_call: args.num_tasks_per_queue_call,
        schedule: args.schedule,
        name: args.name.clone(),
        current_exec_ts,
        current_transaction_id: 0,
        next_transaction_id: 0,
        bump_seed: ctx.bumps.cron_job,
        removed_from_queue: false,
        num_transactions: 0,
        next_schedule_task: ctx.accounts.task.key(),
    });
    ctx.accounts.user_cron_jobs.next_cron_job_id += 1;
    ctx.accounts
        .cron_job_name_mapping
        .set_inner(CronJobNameMappingV0 {
            cron_job: ctx.accounts.cron_job.key(),
            name: args.name.clone(),
            bump_seed: ctx.bumps.cron_job_name_mapping,
        });

    let queue_tx = compile_schedule_transaction(
        &ctx.accounts.cron_job,
        ctx.accounts.cron_job.key(),
        ctx.accounts.task_return_account_1.key(),
        ctx.accounts.task_return_account_2.key(),
        ctx.accounts.task.key(),
    )?;

    transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            Transfer {
                from: ctx.accounts.payer.to_account_info(),
                to: ctx.accounts.task_return_account_1.to_account_info(),
            },
        ),
        // Allocate enough rent for one tx
        Rent::get()?.minimum_balance(1024),
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
            free_tasks: effective_tasks_per_queue_call(&ctx.accounts.cron_job) + 1,
            id: ctx
                .accounts
                .task_queue
                .next_available_task_id()
                .ok_or_else(|| error!(ErrorCode::TaskQueueFull))?,
            description: format!("queue {}", trunc_name),
        },
    )?;

    Ok(())
}
