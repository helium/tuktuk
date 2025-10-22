use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{
    error::ErrorCode,
    state::{TaskQueueAuthorityV0, TaskQueueDataWrapper, TaskV0, TransactionSourceV0, TriggerV0},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct QueueTaskArgsV0 {
    pub id: u16,
    pub trigger: TriggerV0,
    // Note that you can pass accounts from the remaining accounts to reduce
    // the size of the transaction
    pub transaction: TransactionSourceV0,
    pub crank_reward: Option<u64>,
    // Number of free tasks to append to the end of the accounts. This allows
    // you to easily add new tasks
    pub free_tasks: u8,
    // Description of the task. Useful for debugging and logging
    pub description: String,
}

#[derive(Accounts)]
#[instruction(args: QueueTaskArgsV0)]
pub struct QueueTaskV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub queue_authority: Signer<'info>,
    #[account(
        seeds = [b"task_queue_authority", task_queue.key().as_ref(), queue_authority.key().as_ref()],
        bump = task_queue_authority.bump_seed,
    )]
    pub task_queue_authority: Account<'info, TaskQueueAuthorityV0>,
    /// CHECK: We manually deserialize this using TaskQueueDataWrapper for memory efficiency
    #[account(mut)]
    pub task_queue: UncheckedAccount<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + std::mem::size_of::<TaskV0>() + args.transaction.size() + args.description.len() + 60,
        seeds = [b"task".as_ref(), task_queue.key().as_ref(), &args.id.to_le_bytes()[..]],
        bump,
    )]
    pub task: Account<'info, TaskV0>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<QueueTaskV0>, args: QueueTaskArgsV0) -> Result<()> {
    // Use memory-efficient wrapper to avoid deserializing the entire task queue
    let task_queue_account_info = ctx.accounts.task_queue.to_account_info();
    let mut task_queue_data = task_queue_account_info.try_borrow_mut_data()?;
    let mut task_queue = TaskQueueDataWrapper::new(*task_queue_data)?;

    // Validate constraints that were removed from the account struct
    require!(
        !task_queue.task_exists(args.id),
        ErrorCode::TaskAlreadyExists
    );
    require!(
        args.id < task_queue.header().capacity,
        ErrorCode::InvalidTaskId
    );

    require_gte!(
        task_queue.header().capacity,
        (args.free_tasks + 1) as u16,
        ErrorCode::FreeTasksGreaterThanCapacity
    );
    require_gte!(
        40,
        args.description.len(),
        ErrorCode::InvalidDescriptionLength
    );
    let crank_reward = args
        .crank_reward
        .unwrap_or(task_queue.header().min_crank_reward);
    require_gte!(crank_reward, task_queue.header().min_crank_reward);

    let mut transaction = args.transaction;
    if let TransactionSourceV0::CompiledV0(ref mut compiled_tx) = transaction {
        compiled_tx
            .accounts
            .extend(ctx.remaining_accounts.iter().map(|a| a.key()));
    }
    ctx.accounts.task.set_inner(TaskV0 {
        free_tasks: args.free_tasks,
        description: args.description,
        task_queue: ctx.accounts.task_queue.key(),
        id: args.id,
        trigger: args.trigger,
        rent_amount: 0,
        crank_reward,
        rent_refund: ctx.accounts.payer.key(),
        transaction,
        bump_seed: ctx.bumps.task,
        queued_at: Clock::get()?.unix_timestamp,
    });

    // Update the task queue bitmap and metadata
    task_queue.set_task_exists(args.id, true);
    task_queue.header_mut().updated_at = Clock::get()?.unix_timestamp;
    task_queue.save()?;

    // Drop the borrow
    drop(task_queue_data);

    let rented_amount = ctx.accounts.task.to_account_info().lamports();
    ctx.accounts.task.rent_amount = rented_amount;

    transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            Transfer {
                from: ctx.accounts.payer.to_account_info(),
                to: ctx.accounts.task.to_account_info(),
            },
        ),
        crank_reward,
    )?;

    Ok(())
}
