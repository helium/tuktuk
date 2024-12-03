use anchor_lang::prelude::*;

use crate::{
    error::ErrorCode,
    resize_to_fit::resize_to_fit,
    state::{CompiledTransactionV0, TaskQueueV0, TaskV0, TriggerV0},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct QueueTaskArgsV0 {
    pub id: u16,
    pub trigger: TriggerV0,
    // Note that you can pass accounts from the remaining accounts to reduce
    // the size of the transaction
    pub transaction: CompiledTransactionV0,
    pub crank_reward: Option<u64>,
    // Number of free tasks to append to the end of the accounts. This allows
    // you to easily add new tasks
    pub free_tasks: u8,
}

#[derive(Accounts)]
#[instruction(args: QueueTaskArgsV0)]
pub struct QueueTaskV0<'info> {
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
        constraint = !task_queue.task_exists(args.id) @ ErrorCode::TaskAlreadyExists,
        constraint = args.id < task_queue.capacity,
        seeds = [b"task".as_ref(), task_queue.key().as_ref(), &args.id.to_le_bytes()[..]],
        bump,
    )]
    pub task: Box<Account<'info, TaskV0>>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<QueueTaskV0>, args: QueueTaskArgsV0) -> Result<()> {
    let mut transaction = args.transaction.clone();
    transaction
        .accounts
        .extend(ctx.remaining_accounts.iter().map(|a| a.key()));
    ctx.accounts.task.set_inner(TaskV0 {
        free_tasks: args.free_tasks,
        task_queue: ctx.accounts.task_queue.key(),
        id: args.id,
        trigger: args.trigger,
        crank_reward: args
            .crank_reward
            .unwrap_or(ctx.accounts.task_queue.default_crank_reward),
        rent_refund: ctx.accounts.payer.key(),
        transaction,
        bump_seed: ctx.bumps.task,
        queued_at: Clock::get()?.unix_timestamp,
    });
    ctx.accounts.task_queue.set_task_exists(args.id, true);
    ctx.accounts.task_queue.updated_at = Clock::get()?.unix_timestamp;

    resize_to_fit(
        &ctx.accounts.payer.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.task,
    )?;

    Ok(())
}
