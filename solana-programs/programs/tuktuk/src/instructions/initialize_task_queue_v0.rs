use anchor_lang::{prelude::*, solana_program::hash::hash};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::state::{TaskQueueNameMappingV0, TaskQueueV0, TuktukConfigV0};

pub const TESTING: bool = std::option_env!("TESTING").is_some();

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct InitializeTaskQueueArgsV0 {
    pub crank_reward: u64,
    pub name: String,
    pub capacity: u16,
}

fn hash_name(name: &str) -> [u8; 32] {
    hash(name.as_bytes()).to_bytes()
}

#[derive(Accounts)]
#[instruction(args: InitializeTaskQueueArgsV0)]
pub struct InitializeTaskQueueV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        mut,
        has_one = network_mint
    )]
    pub tuktuk_config: Box<Account<'info, TuktukConfigV0>>,
    pub network_mint: Box<Account<'info, Mint>>,
    /// CHECK: Is getting set by signer
    pub update_authority: UncheckedAccount<'info>,
    /// CHECK: Is getting set by signer
    pub queue_authority: UncheckedAccount<'info>,
    #[account(
      init,
      payer = payer,
      seeds = ["task_queue".as_bytes(), tuktuk_config.key().as_ref(), &tuktuk_config.next_task_queue_id.to_le_bytes()[..]],
      bump,
      space = 60 + std::mem::size_of::<TaskQueueV0>() + args.name.len() + ((args.capacity + 7) / 8) as usize,
    )]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    #[account(
        init,
        payer = payer,
        space = TaskQueueNameMappingV0::INIT_SPACE,
        seeds = [
            "task_queue_name_mapping".as_bytes(),
            tuktuk_config.key().as_ref(),
            &hash_name(args.name.as_str())
        ],
        bump
    )]
    pub task_queue_name_mapping: Box<Account<'info, TaskQueueNameMappingV0>>,
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = network_mint,
        associated_token::authority = task_queue,
    )]
    pub rewards_source: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializeTaskQueueV0>, args: InitializeTaskQueueArgsV0) -> Result<()> {
    require_gte!(32, args.name.len());

    ctx.accounts.task_queue.set_inner(TaskQueueV0 {
        rewards_source: ctx.accounts.rewards_source.key(),
        id: ctx.accounts.tuktuk_config.next_task_queue_id,
        tuktuk_config: ctx.accounts.tuktuk_config.key(),
        update_authority: ctx.accounts.update_authority.key(),
        queue_authority: ctx.accounts.queue_authority.key(),
        default_crank_reward: args.crank_reward,
        capacity: args.capacity,
        task_bitmap: vec![0; ((args.capacity + 7) / 8) as usize],
        name: args.name.clone(),
        bump_seed: ctx.bumps.task_queue,
    });
    ctx.accounts
        .task_queue_name_mapping
        .set_inner(TaskQueueNameMappingV0 {
            task_queue: ctx.accounts.task_queue.key(),
            name: args.name,
            bump_seed: ctx.bumps.task_queue_name_mapping,
        });
    ctx.accounts.tuktuk_config.next_task_queue_id += 1;
    Ok(())
}
