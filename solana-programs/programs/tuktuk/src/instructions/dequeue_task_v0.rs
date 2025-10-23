use anchor_lang::prelude::*;

use crate::state::{TaskQueueAuthorityV0, TaskQueueDataWrapper, TaskV0};

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
    pub task_queue_authority: Account<'info, TaskQueueAuthorityV0>,
    /// CHECK: We manually deserialize this using TaskQueueDataWrapper for memory efficiency
    #[account(mut)]
    pub task_queue: UncheckedAccount<'info>,
    #[account(
        mut,
        close = rent_refund,
        has_one = rent_refund,
        has_one = task_queue,
    )]
    pub task: Account<'info, TaskV0>,
}

pub fn handler(ctx: Context<DequeuetaskV0>) -> Result<()> {
    let task_queue_account_info = ctx.accounts.task_queue.to_account_info();
    let mut task_queue_data = task_queue_account_info.try_borrow_mut_data()?;
    let mut task_queue = TaskQueueDataWrapper::new(*task_queue_data)?;
    task_queue.set_task_exists(ctx.accounts.task.id, false);
    Ok(())
}
