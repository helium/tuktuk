use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{close_account, transfer, CloseAccount, Mint, Token, TokenAccount, Transfer},
};

use super::hash_name;
use crate::{
    error::ErrorCode,
    state::{TaskQueueNameMappingV0, TaskQueueV0, TuktukConfigV0},
    task_queue_seeds,
};

#[derive(Accounts)]
pub struct CloseTaskQueueV0<'info> {
    /// CHECK: Just getting sol
    #[account(mut)]
    pub refund: AccountInfo<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub update_authority: Signer<'info>,
    #[account(
        mut,
        has_one = network_mint,
    )]
    pub tuktuk_config: Box<Account<'info, TuktukConfigV0>>,
    pub network_mint: Box<Account<'info, Mint>>,
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
        associated_token::mint = network_mint,
        associated_token::authority = task_queue,
    )]
    pub rewards_source: Box<Account<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = network_mint,
        associated_token::authority = refund,
    )]
    pub rewards_refund: Box<Account<'info, TokenAccount>>,
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
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub fn handler(ctx: Context<CloseTaskQueueV0>) -> Result<()> {
    if ctx.accounts.task_queue.id == ctx.accounts.tuktuk_config.min_task_queue_id {
        ctx.accounts.tuktuk_config.min_task_queue_id = ctx.accounts.task_queue.id + 1;
    }

    transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.rewards_source.to_account_info(),
                to: ctx.accounts.rewards_refund.to_account_info(),
                authority: ctx.accounts.task_queue.to_account_info(),
            },
            &[task_queue_seeds!(ctx.accounts.task_queue)],
        ),
        ctx.accounts.rewards_source.amount,
    )?;
    close_account(CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        CloseAccount {
            account: ctx.accounts.rewards_source.to_account_info(),
            destination: ctx.accounts.refund.to_account_info(),
            authority: ctx.accounts.task_queue.to_account_info(),
        },
        &[task_queue_seeds!(ctx.accounts.task_queue)],
    ))?;
    Ok(())
}
