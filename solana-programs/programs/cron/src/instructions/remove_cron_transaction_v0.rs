use anchor_lang::prelude::*;

use crate::state::{CronJobTransactionV0, CronJobV0};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct RemoveCronTransactionArgsV0 {
    pub index: u32,
}

#[derive(Accounts)]
#[instruction(args: RemoveCronTransactionArgsV0)]
pub struct RemoveCronTransactionV0<'info> {
    #[account(mut)]
    pub rent_refund: Signer<'info>,
    pub authority: Signer<'info>,
    #[account(mut, has_one = authority)]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    #[account(
        mut,
        close = rent_refund,
        has_one = cron_job,
        seeds = [b"cron_job_transaction", cron_job.key().as_ref(), &args.index.to_le_bytes()[..]],
        bump = cron_job_transaction.bump_seed,
    )]
    pub cron_job_transaction: Box<Account<'info, CronJobTransactionV0>>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<RemoveCronTransactionV0>,
    args: RemoveCronTransactionArgsV0,
) -> Result<()> {
    if ctx.accounts.cron_job.max_transaction_idx == args.index {
        ctx.accounts.cron_job.max_transaction_idx -= 1;
    }
    Ok(())
}
