use anchor_lang::prelude::*;

use crate::{
    error::ErrorCode,
    state::{TaskQueueAuthorityV0, TaskQueueV0},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct WithdrawTaskQueueSurplusArgsV0 {
    pub amount: u64,
}

#[derive(Accounts)]
pub struct WithdrawTaskQueueSurplusV0<'info> {
    pub update_authority: Signer<'info>,
    /// CHECK: Proven a queue authority of this task queue by the seeds of task_queue_authority
    #[account(mut)]
    pub queue_authority: AccountInfo<'info>,
    #[account(
        seeds = [b"task_queue_authority", task_queue.key().as_ref(), queue_authority.key().as_ref()],
        bump = task_queue_authority.bump_seed,
    )]
    pub task_queue_authority: Box<Account<'info, TaskQueueAuthorityV0>>,
    #[account(
      mut,
      has_one = update_authority,
    )]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
}

pub fn handler(
    ctx: Context<WithdrawTaskQueueSurplusV0>,
    args: WithdrawTaskQueueSurplusArgsV0,
) -> Result<()> {
    let task_queue_info = ctx.accounts.task_queue.to_account_info();
    let min_lamports = Rent::get()?.minimum_balance(task_queue_info.data_len());
    require_gte!(
        task_queue_info
            .lamports()
            .checked_sub(args.amount)
            .ok_or(ErrorCode::TaskQueueInsufficientFunds)?,
        min_lamports,
        ErrorCode::TaskQueueInsufficientFunds
    );

    task_queue_info.sub_lamports(args.amount)?;
    ctx.accounts.queue_authority.add_lamports(args.amount)?;
    Ok(())
}
