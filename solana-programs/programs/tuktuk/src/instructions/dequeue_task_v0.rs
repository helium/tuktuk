use anchor_lang::prelude::*;

use crate::state::{TaskQueueV0, TaskV0};

#[derive(Accounts)]
pub struct DequeuetaskV0<'info> {
    pub queue_authority: Signer<'info>,
    /// CHECK: Via has one
    #[account(mut)]
    pub rent_refund: AccountInfo<'info>,
    #[account(mut, has_one = queue_authority)]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    #[account(
        mut,
        close = rent_refund,
        has_one = rent_refund,
        seeds = [b"task".as_ref(), task_queue.key().as_ref(), &task.id.to_le_bytes()[..]],
        bump = task.bump_seed,
    )]
    pub task: Box<Account<'info, TaskV0>>,
}

pub fn handler(ctx: Context<DequeuetaskV0>) -> Result<()> {
    ctx.accounts
        .task_queue
        .set_task_exists(ctx.accounts.task.id, false);
    Ok(())
}
