use anchor_lang::prelude::*;

use crate::state::{TaskQueueAuthorityV0, TaskQueueV0, TaskV0};

#[derive(Accounts)]
pub struct DequeuetaskV0<'info> {
    pub queue_authority: Signer<'info>,
    /// CHECK: Via has one
    #[account(mut)]
    pub rent_refund: AccountInfo<'info>,
    #[account(
        seeds = [b"task_queue_authority", task_queue.key().as_ref(), queue_authority.key().as_ref()],
        bump = task_queue_authority.bump_seed,
    )]
    pub task_queue_authority: Box<Account<'info, TaskQueueAuthorityV0>>,
    #[account(mut)]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    #[account(
        mut,
        close = rent_refund,
        has_one = rent_refund,
        has_one = task_queue,
    )]
    pub task: Box<Account<'info, TaskV0>>,
}

pub fn handler(ctx: Context<DequeuetaskV0>) -> Result<()> {
    ctx.accounts
        .task_queue
        .set_task_exists(ctx.accounts.task.id, false);
    Ok(())
}
