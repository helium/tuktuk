use anchor_lang::prelude::*;

use crate::state::{TaskQueueAuthorityV0, TaskQueueV0};

#[derive(Accounts)]
pub struct AddQueueAuthorityV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub update_authority: Signer<'info>,
    /// CHECK: Just bein set as the authority
    pub queue_authority: AccountInfo<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + std::mem::size_of::<TaskQueueAuthorityV0>() + 60,
        seeds = [b"task_queue_authority", task_queue.key().as_ref(), queue_authority.key().as_ref()],
        bump,
    )]
    pub task_queue_authority: Box<Account<'info, TaskQueueAuthorityV0>>,
    #[account(
      mut,
      has_one = update_authority,
    )]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<AddQueueAuthorityV0>) -> Result<()> {
    ctx.accounts
        .task_queue_authority
        .set_inner(TaskQueueAuthorityV0 {
            queue_authority: ctx.accounts.queue_authority.key(),
            task_queue: ctx.accounts.task_queue.key(),
            bump_seed: ctx.bumps.task_queue_authority,
        });
    Ok(())
}
