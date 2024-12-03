use anchor_lang::{
    prelude::*,
    solana_program::{self, instruction::Instruction},
    system_program,
};
use anchor_spl::token::{transfer, Token, Transfer};

use crate::{
    error::ErrorCode,
    state::{CompiledTransactionV0, TaskQueueV0, TaskV0, TriggerV0},
    task_queue_seeds, task_seeds,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RunTaskReturnV0 {
    pub tasks: Vec<TaskReturnV0>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct TaskReturnV0 {
    pub trigger: TriggerV0,
    // Note that you can pass accounts from the remaining accounts to reduce
    // the size of the transaction
    pub transaction: CompiledTransactionV0,
    pub crank_reward: Option<u64>,
    // Number of free tasks to append to the end of the accounts. This allows
    // you to easily add new tasks
    pub free_tasks: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RunTaskArgsV0 {
    pub free_task_ids: Vec<u16>,
}

#[derive(Accounts)]
pub struct RunTaskV0<'info> {
    // Having this here allows us to make sure the payer isn't used in
    // any of the sub instructions
    pub payer: Signer<'info>,
    /// CHECK: Via has one
    #[account(mut)]
    pub rent_refund: AccountInfo<'info>,
    #[account(
        mut,
        has_one = rewards_source,
    )]
    pub task_queue: Account<'info, TaskQueueV0>,
    #[account(
        mut,
        has_one = task_queue,
        has_one = rent_refund,
        close = rent_refund,
        constraint = task.trigger.is_active()? @ ErrorCode::TaskNotReady,
    )]
    pub task: Box<Account<'info, TaskV0>>,
    /// CHECK: Via CPI
    #[account(mut)]
    pub rewards_source: AccountInfo<'info>,
    /// CHECK: Via CPI
    #[account(mut)]
    pub rewards_destination: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, RunTaskV0<'info>>,
    args: RunTaskArgsV0,
) -> Result<()> {
    ctx.accounts.task_queue.updated_at = Clock::get()?.unix_timestamp;
    let transaction = &ctx.accounts.task.transaction;

    let prefix: Vec<&[u8]> = vec![b"custom", ctx.accounts.task.task_queue.as_ref()];
    // Need to convert to &[&[u8]] because invoke_signed expects that
    let signers_inner_u8: Vec<Vec<&[u8]>> = transaction
        .signer_seeds
        .iter()
        .map(|s| {
            let mut clone = prefix.clone();
            clone.extend(s.iter().map(|v| v.as_slice()).collect::<Vec<&[u8]>>());

            clone
        })
        .collect();
    let signers = signers_inner_u8
        .iter()
        .map(|s| s.as_slice())
        .collect::<Vec<&[&[u8]]>>();

    let signer_addresses = signers
        .iter()
        .map(|s| Pubkey::create_program_address(s, ctx.program_id).unwrap())
        .collect::<std::collections::HashSet<Pubkey>>();

    // Validate txn
    for (index, account) in ctx.remaining_accounts[..transaction.accounts.len()]
        .iter()
        .enumerate()
    {
        require_neq!(
            account.key(),
            ctx.accounts.payer.key(),
            ErrorCode::InvalidAccount
        );
        let signers_end = transaction.num_ro_signers + transaction.num_rw_signers;
        // It is okay if an account not labeled as a signer is a signer.
        // For example, if an account being passed is a fee payer
        if index < signers_end as usize {
            require!(
                account.is_signer || signer_addresses.contains(&account.key()),
                ErrorCode::InvalidSigner,
            );
        }

        let is_writable = index < transaction.num_rw as usize
            || (index >= signers_end as usize
                && index < (signers_end + transaction.num_rw) as usize);
        // While it would be nice to validate non-writable accounts aren't writable,
        // this is not possible. We can't tell who the tx fee payer is, so they may be writable
        // because of that. Or they may be the refund target.
        if is_writable {
            require!(account.is_writable, ErrorCode::InvalidWritable);
        }

        require_eq!(
            *account.key,
            transaction.accounts[index],
            ErrorCode::InvalidAccount
        );
    }

    // Validate that all free task accounts are empty
    let free_tasks_start_index = transaction.accounts.len();
    for i in 0..ctx.accounts.task.free_tasks {
        let free_task_index = free_tasks_start_index + i as usize;
        let free_task_account = &ctx.remaining_accounts[free_task_index];
        require!(
            free_task_account.data_is_empty(),
            ErrorCode::FreeTaskAccountNotEmpty
        );
    }

    let mut tasks = Vec::new();
    for ix in &transaction.instructions {
        let mut accounts = Vec::new();
        let mut account_infos = Vec::new();

        for i in &ix.accounts {
            let acct = ctx.remaining_accounts[*i as usize].clone();
            accounts.push(acct.clone());
            account_infos.push(AccountMeta {
                pubkey: acct.key(),
                is_signer: acct.is_signer || signer_addresses.contains(&acct.key()),
                is_writable: acct.is_writable,
            })
        }

        solana_program::program::invoke_signed(
            &Instruction {
                program_id: *ctx.remaining_accounts[ix.program_id_index as usize].key,
                accounts: account_infos,
                data: ix.data.clone(),
            },
            accounts.as_slice(),
            &signers,
        )?;
        if let Some((_, return_data)) = solana_program::program::get_return_data() {
            let queue_task_return = RunTaskReturnV0::deserialize(&mut return_data.as_slice())?;
            tasks.extend(queue_task_return.tasks);
        }
    }

    transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.rewards_source.to_account_info(),
                to: ctx.accounts.rewards_destination.to_account_info(),
                authority: ctx.accounts.task_queue.to_account_info(),
            },
            &[task_queue_seeds!(ctx.accounts.task_queue)],
        ),
        ctx.accounts.task.crank_reward,
    )?;

    let mut free_task_ids = args.free_task_ids.clone();
    // Reverse so we're popping from the end
    free_task_ids.reverse();

    for (i, task) in tasks.iter().enumerate() {
        let free_task_index = free_tasks_start_index + i;
        let free_task_account = &ctx.remaining_accounts[free_task_index];
        let task_queue = &mut ctx.accounts.task_queue;
        let task_queue_key = task_queue.key();

        let task_id = free_task_ids.pop().unwrap();

        // Verify the PDA
        let seeds = [b"task", task_queue_key.as_ref(), &task_id.to_le_bytes()];
        let (key, bump_seed) = Pubkey::find_program_address(&seeds, ctx.program_id);
        require_eq!(key, free_task_account.key(), ErrorCode::InvalidTaskPDA);

        // Initialize the task
        let task_data = TaskV0 {
            task_queue: task_queue_key,
            id: task_id, // Use the provided task_id instead
            rent_refund: task_queue_key,
            trigger: task.trigger.clone(),
            transaction: task.transaction.clone(),
            crank_reward: task.crank_reward.unwrap_or(task_queue.default_crank_reward),
            bump_seed,
            queued_at: Clock::get()?.unix_timestamp,
            free_tasks: task.free_tasks,
        };

        ctx.accounts.task_queue.set_task_exists(task_data.id, true);

        let task_size = ctx.accounts.task.to_account_info().data_len();
        let lamports = Rent::get()?.minimum_balance(task_size);

        // Create and allocate the account
        let task_queue_info = ctx.accounts.task_queue.to_account_info();
        let task_queue_min_lamports = Rent::get()?.minimum_balance(task_queue_info.data_len() + 60);
        require_gt!(
            task_queue_info.lamports(),
            task_queue_min_lamports + lamports,
            ErrorCode::TaskQueueInsufficientFunds
        );

        system_program::allocate(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Allocate {
                    account_to_allocate: free_task_account.to_account_info(),
                },
                &[task_seeds!(task_data)],
            ),
            task_size as u64,
        )?;

        system_program::assign(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Assign {
                    account_to_assign: free_task_account.to_account_info(),
                },
                &[task_seeds!(task_data)],
            ),
            ctx.program_id,
        )?;

        **task_queue_info.try_borrow_mut_lamports()? -= lamports;
        **free_task_account.try_borrow_mut_lamports()? += lamports;

        let mut data = free_task_account.try_borrow_mut_data()?;
        task_data.try_serialize(&mut &mut data[..])?;
    }

    ctx.accounts
        .task_queue
        .set_task_exists(ctx.accounts.task.id, false);

    Ok(())
}
