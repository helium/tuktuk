use anchor_lang::prelude::*;

use super::hash_name;
use crate::{
    error::ErrorCode,
    state::{TaskQueueNameMappingV0, TaskQueueV0, TuktukConfigV0},
};

#[derive(Accounts)]
pub struct CloseTaskQueueV0<'info> {
    /// CHECK: Just getting sol
    #[account(mut)]
    pub refund: AccountInfo<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub update_authority: Signer<'info>,
    #[account(mut)]
    pub tuktuk_config: Box<Account<'info, TuktukConfigV0>>,
    #[account(
        mut,
        close = refund,
        has_one = update_authority,
        has_one = tuktuk_config,
        constraint = task_queue.task_bitmap.iter().all(|&bit| bit == 0) @ ErrorCode::TaskQueueNotEmpty,
    )]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    #[account(
        mut,
        close = refund,
        seeds = [
            "task_queue_name_mapping".as_bytes(),
            task_queue.tuktuk_config.as_ref(),
            &hash_name(task_queue.name.as_str())
        ],
        bump = task_queue_name_mapping.bump_seed
    )]
    pub task_queue_name_mapping: Account<'info, TaskQueueNameMappingV0>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CloseTaskQueueV0>) -> Result<()> {
    if ctx.accounts.task_queue.id == ctx.accounts.tuktuk_config.min_task_queue_id {
        ctx.accounts.tuktuk_config.min_task_queue_id = ctx.accounts.task_queue.id + 1;
    }

    if ctx.accounts.task_queue.id == ctx.accounts.tuktuk_config.next_task_queue_id - 1 {
        ctx.accounts.tuktuk_config.next_task_queue_id -= 1;
    }

    Ok(())
}
