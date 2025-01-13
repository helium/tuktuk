use anchor_lang::{
    prelude::*,
    solana_program::{
        self,
        hash::hash,
        instruction::Instruction,
        sysvar::instructions::{
            load_current_index_checked, load_instruction_at_checked, ID as IX_ID,
        },
    },
    system_program,
};

use crate::{
    error::ErrorCode,
    resize_to_fit::IgnoreWriter,
    state::{CompiledTransactionV0, TaskQueueV0, TaskV0, TransactionSourceV0, TriggerV0},
    task_seeds, utils,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RunTaskReturnV0 {
    pub tasks: Vec<TaskReturnV0>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RemoteTaskTransactionV0 {
    pub task: Pubkey,
    pub task_queued_at: i64,
    pub remaining_accounts_hash: [u8; 32],
    pub num_accounts: u8,
    // NOTE: The `.accounts` should be empty here, it's instead done via
    // remaining_accounts_hash
    pub transaction: CompiledTransactionV0,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct TaskReturnV0 {
    pub trigger: TriggerV0,
    // Note that you can pass accounts from the remaining accounts to reduce
    // the size of the transaction
    pub transaction: TransactionSourceV0,
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
    #[account(mut)]
    pub crank_turner: Signer<'info>,
    /// CHECK: Via has one
    #[account(mut)]
    pub rent_refund: AccountInfo<'info>,
    #[account(mut)]
    pub task_queue: Account<'info, TaskQueueV0>,
    #[account(
        mut,
        has_one = task_queue,
        has_one = rent_refund,
        close = rent_refund,
        constraint = task.trigger.is_active()? @ ErrorCode::TaskNotReady,
    )]
    pub task: Box<Account<'info, TaskV0>>,
    pub system_program: Program<'info, System>,

    /// CHECK: The address check is needed because otherwise
    /// the supplied Sysvar could be anything else.
    /// The Instruction Sysvar has not been implemented
    /// in the Anchor framework yet, so this is the safe approach.
    #[account(address = IX_ID)]
    pub sysvar_instructions: AccountInfo<'info>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, RunTaskV0<'info>>,
    args: RunTaskArgsV0,
) -> Result<()> {
    ctx.accounts.task_queue.updated_at = Clock::get()?.unix_timestamp;
    let remaining_accounts = ctx.remaining_accounts;
    let transaction_source = ctx.accounts.task.transaction.clone();
    let transaction = match transaction_source {
        TransactionSourceV0::CompiledV0(compiled_tx) => compiled_tx,
        TransactionSourceV0::RemoteV0 { signer, .. } => {
            let ix_index =
                load_current_index_checked(&ctx.accounts.sysvar_instructions.to_account_info())?;
            let ix: Instruction = load_instruction_at_checked(
                ix_index.checked_sub(1).unwrap() as usize,
                &ctx.accounts.sysvar_instructions,
            )?;
            let data = utils::ed25519::verify_ed25519_ix(&ix, signer.to_bytes().as_slice())?;
            let mut remote_tx = RemoteTaskTransactionV0::deserialize(&mut data.as_slice())?;
            require_eq!(
                remote_tx.task,
                ctx.accounts.task.key(),
                ErrorCode::InvalidTask
            );
            require_eq!(
                remote_tx.task_queued_at,
                ctx.accounts.task.queued_at,
                ErrorCode::InvalidTaskQueuedAt
            );
            // Verify all the remaining accounts and update the remote_tx.transaction.accounts
            let remaining_accounts_hash = hash(
                &remaining_accounts[..remote_tx.num_accounts as usize]
                    .iter()
                    .map(|acc| {
                        let mut data = Vec::with_capacity(34); // 32 bytes for pubkey + 2 bytes for flags
                        data.extend_from_slice(&acc.key.to_bytes());
                        data.push(if acc.is_writable { 1 } else { 0 });
                        data.push(if acc.is_signer { 1 } else { 0 });
                        remote_tx.transaction.accounts.push(*acc.key);
                        data
                    })
                    .collect::<Vec<_>>()
                    .concat(),
            );
            require!(
                remaining_accounts_hash.to_bytes() == remote_tx.remaining_accounts_hash,
                ErrorCode::InvalidRemainingAccountsHash
            );
            remote_tx.transaction
        }
    };
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
    for (index, account) in remaining_accounts[..transaction.accounts.len()]
        .iter()
        .enumerate()
    {
        require_neq!(
            account.key(),
            ctx.accounts.crank_turner.key(),
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
        let free_task_account = &remaining_accounts[free_task_index];
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
            let acct = remaining_accounts[*i as usize].clone();
            accounts.push(acct.clone());
            account_infos.push(AccountMeta {
                pubkey: acct.key(),
                is_signer: acct.is_signer || signer_addresses.contains(&acct.key()),
                is_writable: acct.is_writable,
            })
        }

        solana_program::program::invoke_signed(
            &Instruction {
                program_id: *remaining_accounts[ix.program_id_index as usize].key,
                accounts: account_infos,
                data: ix.data.clone(),
            },
            accounts.as_slice(),
            &signers,
        )?;
        if let Some((_, return_data)) = solana_program::program::get_return_data() {
            msg!("Return data: {:?}", return_data);
            let queue_task_return = RunTaskReturnV0::deserialize(&mut return_data.as_slice())?;
            msg!("Parsed");
            tasks.extend(queue_task_return.tasks);
        }
    }

    let reward = ctx.accounts.task.crank_reward;
    // 5% fee
    let protocol_fee = reward.checked_mul(5).unwrap().checked_div(100).unwrap();
    let task_fee = reward.saturating_sub(protocol_fee);
    **ctx
        .accounts
        .task
        .to_account_info()
        .try_borrow_mut_lamports()? -= reward;
    **ctx.accounts.crank_turner.try_borrow_mut_lamports()? += task_fee;
    **ctx
        .accounts
        .task_queue
        .to_account_info()
        .try_borrow_mut_lamports()? += protocol_fee;
    ctx.accounts.task_queue.uncollected_protocol_fees += protocol_fee;

    let mut free_task_ids = args.free_task_ids.clone();
    // Reverse so we're popping from the end
    free_task_ids.reverse();

    for (i, task) in tasks.iter().enumerate() {
        let free_task_index = free_tasks_start_index + i;
        let free_task_account = &remaining_accounts[free_task_index];
        let task_queue = &mut ctx.accounts.task_queue;
        let task_queue_key = task_queue.key();

        let task_id = free_task_ids.pop().unwrap();

        // Verify the PDA
        let seeds = [b"task", task_queue_key.as_ref(), &task_id.to_le_bytes()];
        let (key, bump_seed) = Pubkey::find_program_address(&seeds, ctx.program_id);
        require_eq!(key, free_task_account.key(), ErrorCode::InvalidTaskPDA);

        // Initialize the task
        let mut task_data = TaskV0 {
            task_queue: task_queue_key,
            id: task_id, // Use the provided task_id instead
            rent_refund: task_queue_key,
            trigger: task.trigger.clone(),
            transaction: task.transaction.clone(),
            crank_reward: task.crank_reward.unwrap_or(task_queue.min_crank_reward),
            bump_seed,
            queued_at: Clock::get()?.unix_timestamp,
            free_tasks: task.free_tasks,
            rent_amount: 0,
        };

        ctx.accounts.task_queue.set_task_exists(task_data.id, true);

        let writer = &mut IgnoreWriter { total: 0 };
        task_data.try_serialize(writer)?;
        // Descriminator + extra padding
        let task_size = writer.total + 8 + 60;
        let lamports = Rent::get()?.minimum_balance(task_size);
        task_data.rent_amount = lamports;

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

        let task_info = ctx.accounts.task.to_account_info();
        let task_remaining_lamports = task_info.lamports();
        let lamports_from_task = task_remaining_lamports.min(lamports);
        let lamports_needed_from_queue = lamports.saturating_sub(lamports_from_task);

        if lamports_from_task > 0 {
            **task_info.try_borrow_mut_lamports()? -= lamports_from_task;
            **free_task_account.try_borrow_mut_lamports()? += lamports_from_task;
        }

        if lamports_needed_from_queue > 0 {
            **task_queue_info.try_borrow_mut_lamports()? -= lamports_needed_from_queue;
            **free_task_account.try_borrow_mut_lamports()? += lamports_needed_from_queue;
        }

        let mut data = free_task_account.try_borrow_mut_data()?;
        task_data.try_serialize(&mut &mut data[..])?;
    }

    ctx.accounts
        .task_queue
        .set_task_exists(ctx.accounts.task.id, false);

    Ok(())
}
