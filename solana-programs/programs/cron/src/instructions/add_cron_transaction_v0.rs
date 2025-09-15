use anchor_lang::prelude::*;
use tuktuk_program::TransactionSourceV0;

use crate::{
    resize_to_fit::resize_to_fit,
    state::{CronJobTransactionV0, CronJobV0},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AddCronTransactionArgsV0 {
    pub index: u32,
    pub transaction_source: TransactionSourceV0,
}

#[derive(Accounts)]
#[instruction(args: AddCronTransactionArgsV0)]
pub struct AddCronTransactionV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub authority: Signer<'info>,
    #[account(mut, has_one = authority)]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    #[account(
        init,
        payer = payer,
        seeds = [b"cron_job_transaction", cron_job.key().as_ref(), &args.index.to_le_bytes()[..]],
        bump,
        space = 8 + std::mem::size_of::<CronJobTransactionV0>() + 60,
    )]
    pub cron_job_transaction: Box<Account<'info, CronJobTransactionV0>>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<AddCronTransactionV0>, args: AddCronTransactionArgsV0) -> Result<()> {
    let cron_job = &mut ctx.accounts.cron_job;
    cron_job.next_transaction_id = cron_job.next_transaction_id.max(args.index + 1);
    cron_job.num_transactions += 1;
    let mut transaction = args.transaction_source.clone();
    if let TransactionSourceV0::CompiledV0(mut compiled_tx) = transaction {
        compiled_tx
            .accounts
            .extend(ctx.remaining_accounts.iter().map(|a| a.key()));
        transaction = TransactionSourceV0::CompiledV0(compiled_tx);
    }

    ctx.accounts
        .cron_job_transaction
        .set_inner(CronJobTransactionV0 {
            id: args.index,
            cron_job: ctx.accounts.cron_job.key(),
            transaction,
            bump_seed: ctx.bumps.cron_job_transaction,
        });

    resize_to_fit(
        &ctx.accounts.payer.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.cron_job_transaction,
    )?;

    Ok(())
}
