use anchor_lang::prelude::*;

use crate::state::{TaskQueueAuthorityV0, TaskQueueV0};

#[derive(Accounts)]
pub struct RemoveQueueAuthorityV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: Just being set as the rent refund
    #[account(mut)]
    pub rent_refund: AccountInfo<'info>,
    pub update_authority: Signer<'info>,
    /// CHECK: Just bein set as the authority
    pub queue_authority: AccountInfo<'info>,
    #[account(
        mut,
        close = rent_refund,
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

pub fn handler(ctx: Context<RemoveQueueAuthorityV0>) -> Result<()> {
    ctx.accounts.task_queue.num_queue_authorities -= 1;
    Ok(())
}
