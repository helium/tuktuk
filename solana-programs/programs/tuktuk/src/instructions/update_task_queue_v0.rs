use anchor_lang::prelude::*;

use crate::{resize_to_fit::resize_to_fit, state::TaskQueueV0};

pub const TESTING: bool = std::option_env!("TESTING").is_some();

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct UpdateTaskQueueArgsV0 {
    pub min_crank_reward: Option<u64>,
    pub capacity: Option<u16>,
    pub lookup_tables: Option<Vec<Pubkey>>,
    pub update_authority: Option<Pubkey>,
    pub stale_task_age: Option<u32>,
}

#[derive(Accounts)]
#[instruction(args: UpdateTaskQueueArgsV0)]
pub struct UpdateTaskQueueV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub update_authority: Signer<'info>,
    #[account(
      mut,
      has_one = update_authority,
    )]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<UpdateTaskQueueV0>, args: UpdateTaskQueueArgsV0) -> Result<()> {
    if let Some(capacity) = args.capacity {
        require_gte!(capacity, ctx.accounts.task_queue.capacity);
        let old_bitmap = ctx.accounts.task_queue.task_bitmap.clone();
        let new_bitmap_size = ((capacity + 7) / 8) as usize;
        let mut new_bitmap = vec![0; new_bitmap_size];

        // Copy over the existing bitmap data
        for (i, &byte) in old_bitmap.iter().enumerate() {
            if i < new_bitmap_size {
                new_bitmap[i] = byte;
            }
        }

        ctx.accounts.task_queue.task_bitmap = new_bitmap;
        ctx.accounts.task_queue.capacity = capacity;
    }
    if let Some(min_crank_reward) = args.min_crank_reward {
        ctx.accounts.task_queue.min_crank_reward = min_crank_reward;
    }
    if let Some(lookup_tables) = args.lookup_tables {
        ctx.accounts.task_queue.lookup_tables = lookup_tables;
    }
    if let Some(update_authority) = args.update_authority {
        ctx.accounts.task_queue.update_authority = update_authority;
    }
    if let Some(stale_task_age) = args.stale_task_age {
        ctx.accounts.task_queue.stale_task_age = stale_task_age;
    }

    resize_to_fit(
        &ctx.accounts.payer.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.task_queue,
    )?;

    Ok(())
}
