use anchor_lang::prelude::*;

use crate::{
    error::ErrorCode,
    hash_name,
    state::{CronJobNameMappingV0, CronJobV0, UserCronJobsV0},
};

#[derive(Accounts)]
pub struct CloseCronJobV0<'info> {
    /// CHECK: Just getting sol
    #[account(mut)]
    pub rent_refund: AccountInfo<'info>,
    pub authority: Signer<'info>,
    #[account(mut)]
    pub user_cron_jobs: Box<Account<'info, UserCronJobsV0>>,
    #[account(
        mut,
        close = rent_refund,
        has_one = authority,
        has_one = user_cron_jobs,
        constraint = cron_job.num_transactions == 0 @ ErrorCode::CronJobHasTransactions
    )]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    #[account(
        mut,
        close = rent_refund,
        seeds = [
            "cron_job_name_mapping".as_bytes(),
            authority.key().as_ref(),
            &hash_name(cron_job.name.as_str())
        ],
        bump = cron_job_name_mapping.bump_seed
    )]
    pub cron_job_name_mapping: Account<'info, CronJobNameMappingV0>,
    pub system_program: Program<'info, System>,
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
}

pub fn handler(ctx: Context<CloseCronJobV0>) -> Result<()> {
    if ctx.accounts.cron_job.id == ctx.accounts.user_cron_jobs.min_cron_job_id {
        ctx.accounts.user_cron_jobs.min_cron_job_id = ctx.accounts.user_cron_jobs.next_cron_job_id;
    }

    if ctx.accounts.cron_job.id == ctx.accounts.user_cron_jobs.next_cron_job_id - 1 {
        ctx.accounts.user_cron_jobs.next_cron_job_id -= 1;
    }

    let task_return_account_1_lamports = ctx.accounts.task_return_account_1.lamports();
    if task_return_account_1_lamports > 0 && *ctx.accounts.task_return_account_1.owner == crate::ID
    {
        ctx.accounts
            .task_return_account_1
            .sub_lamports(task_return_account_1_lamports)?;
        ctx.accounts
            .rent_refund
            .add_lamports(task_return_account_1_lamports)?;
    }

    let task_return_account_2_lamports = ctx.accounts.task_return_account_2.lamports();
    if task_return_account_2_lamports > 0 && *ctx.accounts.task_return_account_2.owner == crate::ID
    {
        ctx.accounts
            .task_return_account_2
            .sub_lamports(task_return_account_2_lamports)?;
        ctx.accounts
            .rent_refund
            .add_lamports(task_return_account_2_lamports)?;
    }

    Ok(())
}
