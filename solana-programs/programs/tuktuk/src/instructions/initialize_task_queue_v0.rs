use anchor_lang::{
    prelude::*,
    solana_program::hash::hash,
    system_program::{transfer, Transfer},
};

use crate::state::{TaskQueueNameMappingV0, TaskQueueV0, TuktukConfigV0};

pub const TESTING: bool = std::option_env!("TESTING").is_some();

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct InitializeTaskQueueArgsV0 {
    pub min_crank_reward: u64,
    pub name: String,
    pub capacity: u16,
    pub lookup_tables: Vec<Pubkey>,
    pub stale_task_age: u32,
}

pub fn hash_name(name: &str) -> [u8; 32] {
    hash(name.as_bytes()).to_bytes()
}

#[derive(Accounts)]
#[instruction(args: InitializeTaskQueueArgsV0)]
pub struct InitializeTaskQueueV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut)]
    pub tuktuk_config: Box<Account<'info, TuktukConfigV0>>,
    /// CHECK: Is getting set by signer
    pub update_authority: UncheckedAccount<'info>,
    #[account(
      init,
      payer = payer,
      seeds = ["task_queue".as_bytes(), tuktuk_config.key().as_ref(), &tuktuk_config.next_task_queue_id.to_le_bytes()[..]],
      bump,
      space = 60 + std::mem::size_of::<TaskQueueV0>() + args.name.len() + ((args.capacity + 7) / 8) as usize + args.lookup_tables.len() * 32,
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
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializeTaskQueueV0>, args: InitializeTaskQueueArgsV0) -> Result<()> {
    require_gte!(32, args.name.len());

    transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            Transfer {
                from: ctx.accounts.payer.to_account_info(),
                to: ctx.accounts.task_queue.to_account_info(),
            },
        ),
        ctx.accounts.tuktuk_config.min_deposit,
    )?;

    ctx.accounts.task_queue.set_inner(TaskQueueV0 {
        lookup_tables: args.lookup_tables,
        id: ctx.accounts.tuktuk_config.next_task_queue_id,
        tuktuk_config: ctx.accounts.tuktuk_config.key(),
        uncollected_protocol_fees: 0,
        update_authority: ctx.accounts.update_authority.key(),
        reserved: Pubkey::default(),
        min_crank_reward: args.min_crank_reward,
        capacity: args.capacity,
        task_bitmap: vec![0; ((args.capacity + 7) / 8) as usize],
        name: args.name.clone(),
        bump_seed: ctx.bumps.task_queue,
        created_at: Clock::get()?.unix_timestamp,
        updated_at: Clock::get()?.unix_timestamp,
        num_queue_authorities: 0,
        stale_task_age: args.stale_task_age,
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
