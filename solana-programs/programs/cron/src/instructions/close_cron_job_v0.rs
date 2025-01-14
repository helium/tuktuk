use anchor_lang::prelude::*;

use crate::{
    hash_name,
    state::{CronJobNameMappingV0, CronJobV0, UserCronJobsV0},
};

#[derive(Accounts)]
pub struct CloseCronJobV0<'info> {
    /// CHECK: Just getting sol
    #[account(mut)]
    pub refund: AccountInfo<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub authority: Signer<'info>,
    #[account(mut)]
    pub user_cron_jobs: Box<Account<'info, UserCronJobsV0>>,
    #[account(
        mut,
        close = refund,
        has_one = authority,
        has_one = user_cron_jobs,
    )]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    #[account(
        mut,
        close = refund,
        seeds = [
            "cron_job_name_mapping".as_bytes(),
            authority.key().as_ref(),
            &hash_name(cron_job.name.as_str())
        ],
        bump = task_queue_name_mapping.bump_seed
    )]
    pub task_queue_name_mapping: Account<'info, CronJobNameMappingV0>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CloseCronJobV0>) -> Result<()> {
    if ctx.accounts.cron_job.id == ctx.accounts.user_cron_jobs.min_cron_job_id {
        ctx.accounts.user_cron_jobs.min_cron_job_id = ctx.accounts.user_cron_jobs.next_cron_job_id;
    }

    if ctx.accounts.cron_job.id == ctx.accounts.user_cron_jobs.next_cron_job_id - 1 {
        ctx.accounts.user_cron_jobs.next_cron_job_id -= 1;
    }

    Ok(())
}
