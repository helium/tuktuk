use anchor_lang::prelude::*;

use crate::{
    error::ErrorCode,
    resize_to_fit::resize_to_fit,
    state::{CompiledInstructionV0, CompiledTransactionV0, TaskQueueV0, TaskV0, TriggerV0},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct QueueTaskArgsV0 {
    pub id: u16,
    pub trigger: TriggerV0,
    pub transaction: CompiledTransactionArgV0,
    pub crank_reward: Option<u64>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct CompiledTransactionArgV0 {
    // Accounts are ordered as follows:
    // 1. Writable signer accounts
    // 2. Read only signer accounts
    // 3. writable accounts
    // 4. read only accounts
    pub num_rw_signers: u8,
    pub num_ro_signers: u8,
    pub num_rw: u8,
    /// Accounts will come from remaining accounts, which allows for lookup tables
    /// and such to reduce size of txn call here
    pub instructions: Vec<CompiledInstructionV0>,
    pub signer_seeds: Vec<Vec<Vec<u8>>>,
}

#[derive(Accounts)]
#[instruction(args: QueueTaskArgsV0)]
pub struct QueuetaskV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub queue_authority: Signer<'info>,
    #[account(
        mut,
        has_one = queue_authority
    )]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    #[account(
        init,
        payer = payer,
        space = 8 + std::mem::size_of::<TaskV0>() + 60,
        constraint = !task_queue.task_exists(args.id as usize) @ ErrorCode::TaskAlreadyExists,
        constraint = args.id < task_queue.capacity,
        seeds = [b"task".as_ref(), task_queue.key().as_ref(), &args.id.to_le_bytes()[..]],
        bump,
    )]
    pub task: Box<Account<'info, TaskV0>>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<QueuetaskV0>, args: QueueTaskArgsV0) -> Result<()> {
    ctx.accounts.task.set_inner(TaskV0 {
        task_queue: ctx.accounts.task_queue.key(),
        id: args.id,
        trigger: args.trigger,
        crank_reward: args
            .crank_reward
            .unwrap_or(ctx.accounts.task_queue.default_crank_reward),
        rent_refund: ctx.accounts.payer.key(),
        transaction: CompiledTransactionV0 {
            num_rw_signers: args.transaction.num_rw_signers,
            num_ro_signers: args.transaction.num_ro_signers,
            num_rw: args.transaction.num_rw,
            instructions: args.transaction.instructions,
            signer_seeds: args.transaction.signer_seeds,
            accounts: ctx.remaining_accounts.iter().map(|a| a.key()).collect(),
        },
        bump_seed: ctx.bumps.task,
        queued_at: Clock::get()?.unix_timestamp,
    });
    ctx.accounts
        .task_queue
        .set_task_exists(args.id as usize, true);
    ctx.accounts.task_queue.updated_at = Clock::get()?.unix_timestamp;

    resize_to_fit(
        &ctx.accounts.payer.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.task,
    )?;

    Ok(())
}
