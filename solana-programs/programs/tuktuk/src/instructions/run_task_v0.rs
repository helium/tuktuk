use anchor_lang::{
    prelude::*,
    solana_program::{
        self,
        entrypoint::MAX_PERMITTED_DATA_INCREASE,
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
    state::{
        CompiledInstructionV0, CompiledTransactionV0, TaskQueueDataWrapper, TaskV0,
        TransactionSourceV0, TriggerV0,
    },
    task_seeds, utils,
};

// You can either fit the task in a return value directly, or you need to return accounts
// that have their ownership set to this program, and are stuffed with ReturnedTasksV0.
// The account method is useful if you want to return a lot of tasks, and don't want to
// hit the 1000 byte return data limit. This allows you to return 10kb worth of tasks.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RunTaskReturnV0 {
    pub tasks: Vec<TaskReturnV0>,
    pub tasks_accounts: Vec<Pubkey>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct TasksAccountHeaderV0 {
    pub num_tasks: u32,
}

impl TasksAccountHeaderV0 {
    pub fn load<'a>(data: &'a mut &'a [u8]) -> Result<(TasksAccountHeaderV0, TasksIterator<'a>)> {
        let header: TasksAccountHeaderV0 = TasksAccountHeaderV0::deserialize(data)?;
        let num_tasks = header.num_tasks;

        Ok((header, TasksIterator::new(num_tasks, data)))
    }
}

const MEMO_PROGRAM_ID: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

/// Counts the bytes written to it and keeps none of them.
#[derive(Default)]
struct ByteCount(usize);

impl std::io::Write for ByteCount {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// How many bytes a value serializes to, without building the serialization. The allocator hands
/// memory out and never takes it back, and borsh reaches a length by doubling, so measuring by
/// serializing charges the heap several times over for a buffer read once and dropped.
/// (`borsh::object_length` does this too, but arrives in borsh 1 and this builds against 0.10.)
fn serialized_len<T: AnchorSerialize>(value: &T) -> Result<usize> {
    let mut count = ByteCount::default();
    value
        .serialize(&mut count)
        .map_err(|_| error!(ErrorCode::ReturnedTaskTooLarge))?;

    Ok(count.0)
}

// Add new iterator struct for reading tasks
pub struct TasksIterator<'a> {
    data: &'a mut &'a [u8],
    current: usize,
    num_tasks: usize,
}

impl<'a> TasksIterator<'a> {
    pub fn new(num_tasks: u32, data: &'a mut &'a [u8]) -> Self {
        Self {
            data,
            current: 0,
            num_tasks: num_tasks as usize,
        }
    }
}

impl<'a> Iterator for TasksIterator<'a> {
    type Item = TaskReturnV0;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.num_tasks {
            return None;
        }

        let task = TaskReturnV0::deserialize(self.data).ok();
        self.current += 1;
        task
    }
}

// This isn't actually an account, but we want anchor to put it in the IDL and serialize it with a discriminator
#[account]
#[derive(Default)]
pub struct RemoteTaskTransactionV0 {
    // A hash of [task, task_queued_at, ...remaining_accounts]
    pub verification_hash: [u8; 32],
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
    pub description: String,
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
    /// CHECK: We manually deserialize this using TaskQueueDataWrapper for memory efficiency
    #[account(mut)]
    pub task_queue: UncheckedAccount<'info>,
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

struct TaskProcessor<'a, 'info> {
    ctx: Context<'a, 'a, 'a, 'info, RunTaskV0<'info>>,
    free_task_ids: Vec<u16>,
    free_task_index: usize,
    signer_addresses: std::collections::HashSet<Pubkey>,
    signers: Vec<Vec<Vec<u8>>>,
    // Task queue data we need for validation
    min_crank_reward: u64,
    capacity: u16,
    // Changes to make to task queue
    tasks_to_set: Vec<u16>, // Task IDs to set as existing
    queue_lamports_needed: u64,
    // Set when a returned task could not be created because of the free-task id or account the
    // crank turner supplied. Errors raised while processing return data are swallowed, so this
    // is carried out to `handler` and failed there. The turner picks both, and a task's children
    // must not be droppable by picking them badly; a reward or description the returning program
    // chose is that program's fault and is still dropped silently.
    //
    // A shortfall of ids also fails the run when the turner supplied every id the task declared
    // and the returning program asked for more children than that. The run failing leaves the
    // task queued rather than losing a child, which for a recurring task is its own next run, so
    // the loud outcome is the one that can be diagnosed.
    bad_free_task_input: bool,
}

impl<'a, 'info> TaskProcessor<'a, 'info> {
    fn new(
        ctx: Context<'a, 'a, 'a, 'info, RunTaskV0<'info>>,
        transaction: &'a CompiledTransactionV0,
        mut free_task_ids: Vec<u16>,
        min_crank_reward: u64,
        capacity: u16,
    ) -> Result<Self> {
        free_task_ids.reverse();

        let prefix: Vec<Vec<u8>> = vec![
            b"custom".to_vec(),
            ctx.accounts.task.task_queue.as_ref().to_vec(),
        ];
        let signers_inner_u8: Vec<Vec<Vec<u8>>> = transaction
            .signer_seeds
            .iter()
            .map(|s| {
                let mut clone = prefix.clone();
                clone.extend(s.iter().map(|v| v.to_vec()).collect::<Vec<Vec<u8>>>());
                clone
            })
            .collect();

        // Seeds past the prefix come from the task's transaction, so they are not guaranteed to
        // resolve to a valid off-curve address.
        let signer_addresses = signers_inner_u8
            .iter()
            .map(|s| {
                let seeds: Vec<&[u8]> = s.iter().map(|v| v.as_slice()).collect();
                Pubkey::create_program_address(&seeds, ctx.program_id)
                    .map_err(|_| error!(ErrorCode::InvalidSignerSeeds))
            })
            .collect::<Result<_>>()?;

        Ok(Self {
            ctx,
            free_task_ids,
            free_task_index: transaction.accounts.len(),
            signer_addresses,
            signers: signers_inner_u8,
            min_crank_reward,
            capacity,
            tasks_to_set: Vec::new(),
            queue_lamports_needed: 0,
            bad_free_task_input: false,
        })
    }

    fn process_instruction(
        &mut self,
        ix: &CompiledInstructionV0,
        remaining_accounts: &[AccountInfo<'info>],
    ) -> Result<()> {
        // The allocator never frees, so a Vec that grows leaks every intermediate buffer. These
        // hold the instruction's accounts and, for everything but memo, the free tasks too.
        // `ix.accounts` holds indices and may name one account repeatedly, so the reserve is
        // capped at the number of accounts there are to name rather than at how many it names.
        let free_tasks = &self.ctx.remaining_accounts[self.free_task_index..];
        // Resolved here rather than at the extend below, so the reservation knows whether the free
        // tasks are going to be appended at all. The heap never hands a reservation back.
        let program_id = remaining_accounts
            .get(ix.program_id_index as usize)
            .ok_or_else(|| error!(ErrorCode::InvalidAccountIndex))?
            .key;
        let takes_free_tasks = *program_id != MEMO_PROGRAM_ID;
        let capacity = ix.accounts.len().min(remaining_accounts.len())
            + if takes_free_tasks {
                free_tasks.len()
            } else {
                0
            };
        let mut accounts = Vec::with_capacity(capacity);
        let mut account_infos = Vec::with_capacity(capacity);

        msg!("Signer addresses: {:?}", self.signer_addresses);

        for i in &ix.accounts {
            // Indices come from the task's transaction and address a slice whose length the crank
            // turner chooses, so neither side of this bound is fixed by the program.
            let mut acct = remaining_accounts
                .get(*i as usize)
                .ok_or_else(|| error!(ErrorCode::InvalidAccountIndex))?
                .clone();
            // A task may only sign for this queue's own `b"custom"` PDAs. Signer privilege is
            // never inherited from the outer transaction: the crank turner is an arbitrary,
            // untrusted account, and forwarding its signature would let any task drain it.
            let is_signer = self.signer_addresses.contains(&acct.key());
            acct.is_signer = is_signer;

            account_infos.push(AccountMeta {
                pubkey: acct.key(),
                is_signer,
                is_writable: acct.is_writable,
            });
            accounts.push(acct);
        }

        // Pass free tasks as remaining accounts so the task can know which IDs will be used.
        // The memo program is skipped because it expects every account passed to be a signer.
        if takes_free_tasks {
            accounts.extend(free_tasks.iter().cloned());
            account_infos.extend(free_tasks.iter().map(|acct| AccountMeta {
                pubkey: acct.key(),
                is_signer: false,
                is_writable: false,
            }));
        }

        let signer_seeds: Vec<Vec<&[u8]>> = self
            .signers
            .iter()
            .map(|s| s.iter().map(|v| v.as_slice()).collect())
            .collect();

        solana_program::program::invoke_signed(
            &Instruction {
                program_id: *program_id,
                accounts: account_infos,
                data: ix.data.clone(),
            },
            accounts.as_slice(),
            &signer_seeds
                .iter()
                .map(|s| s.as_slice())
                .collect::<Vec<&[&[u8]]>>(),
        )?;
        msg!("Invoked");

        if let Some((return_program_id, return_data)) = solana_program::program::get_return_data() {
            // Only the accounts the instruction itself named. The free tasks appended above are
            // the crank turner's to choose, and a tasks account is the program's to name.
            let named = &accounts[..ix.accounts.len()];
            // A run that cannot place a child it was handed fails, whoever caused it: the
            // alternative is a task that reports success while the work it returned is gone.
            self.process_return_data(&return_program_id, &return_data, named)
                .inspect_err(|e| msg!("Error processing return data: {:?}", e))?;
        }

        Ok(())
    }

    fn process_return_data(
        &mut self,
        return_program_id: &Pubkey,
        return_data: &[u8],
        accounts: &[AccountInfo<'info>],
    ) -> Result<()> {
        let queue_task_return = RunTaskReturnV0::deserialize(&mut &return_data[..])?;

        let mut accounts_set = queue_task_return
            .tasks_accounts
            .into_iter()
            .collect::<std::collections::HashSet<Pubkey>>();

        // Each returned account is read once. Taking the key out of the set as it is matched is
        // what says so: an account list may name the same account more than once, and the tasks
        // in an account are queued for every time it is read.
        let tasks_accounts = accounts
            .iter()
            .filter(|a| accounts_set.remove(a.key))
            .collect::<Vec<_>>();

        for task in queue_task_return.tasks {
            self.create_new_task(task)?;
        }

        for account in tasks_accounts {
            self.process_tasks_account(return_program_id, account)?;
        }

        Ok(())
    }

    fn process_tasks_account(
        &mut self,
        return_program_id: &Pubkey,
        account: &AccountInfo<'info>,
    ) -> Result<()> {
        // A program may only hand us task lists out of accounts it owns, and never out of ours:
        // this program's accounts hold tasks and queues, whose bytes would otherwise be read as a
        // task list.
        require_keys_neq!(
            *return_program_id,
            crate::ID,
            ErrorCode::InvalidTasksAccountOwner
        );
        require_keys_eq!(
            *account.owner,
            *return_program_id,
            ErrorCode::InvalidTasksAccountOwner
        );

        let data = account
            .data
            .try_borrow_mut()
            .map_err(|_| error!(ErrorCode::InvalidAccount))?;
        let mut data_ref = data.as_ref();
        let (_, tasks_iter) = TasksAccountHeaderV0::load(&mut data_ref)?;

        for task in tasks_iter {
            self.create_new_task(task)?;
        }

        Ok(())
    }

    fn create_new_task(&mut self, task: TaskReturnV0) -> Result<()> {
        require_gte!(
            40,
            task.description.len(),
            ErrorCode::InvalidDescriptionLength
        );

        require_gte!(
            task.crank_reward.unwrap_or(self.min_crank_reward),
            self.min_crank_reward,
            ErrorCode::InvalidCrankReward
        );
        // A returned task is funded by the task queue, so `min_crank_reward` is its ceiling as
        // well as its floor: together with the check above, a returned reward is either `None` or
        // exactly `min_crank_reward`.
        require_gte!(
            self.min_crank_reward,
            task.crank_reward.unwrap_or(self.min_crank_reward),
            ErrorCode::CrankRewardExceedsMax
        );
        require_gte!(
            self.capacity,
            task.free_tasks as u16 + 1,
            ErrorCode::FreeTasksGreaterThanCapacity
        );

        // Take the id before the account. Ids and free-task accounts are consumed one per created
        // task and their counts are equal, so an exhausted id list is what says there is no
        // account left to take either.
        let task_id = match self.free_task_ids.pop() {
            Some(id) => id,
            None => {
                self.bad_free_task_input = true;
                return Err(error!(ErrorCode::TooManyReturnedTasks));
            }
        };

        // `handler` requires the account count to equal the named accounts plus the free task ids,
        // and a task is created only after an id has been taken, so this index names one of the
        // accounts the turner passed.
        let free_task_account = &self.ctx.remaining_accounts[self.free_task_index];
        self.free_task_index += 1;
        let task_queue_key = self.ctx.accounts.task_queue.key();

        // Verify the account is empty
        if !free_task_account.data_is_empty() {
            self.bad_free_task_input = true;
            return Err(error!(ErrorCode::FreeTaskAccountNotEmpty));
        }

        let seeds = [b"task", task_queue_key.as_ref(), &task_id.to_le_bytes()];
        let (key, bump_seed) = Pubkey::find_program_address(&seeds, self.ctx.program_id);
        if key != free_task_account.key() {
            self.bad_free_task_input = true;
            return Err(error!(ErrorCode::InvalidTaskPDA));
        }

        let mut task_data = TaskV0 {
            description: task.description,
            task_queue: task_queue_key,
            id: task_id,
            rent_refund: task_queue_key,
            trigger: task.trigger,
            transaction: task.transaction,
            crank_reward: task.crank_reward.unwrap_or(self.min_crank_reward),
            bump_seed,
            queued_at: Clock::get()?.unix_timestamp,
            free_tasks: task.free_tasks,
            rent_amount: 0,
        };

        let task_size = serialized_len(&task_data)? + 8 + 60;
        // The account is grown from nothing by the single realloc below, so a returned task has
        // to fit inside what one realloc may add.
        require_gte!(
            MAX_PERMITTED_DATA_INCREASE,
            task_size,
            ErrorCode::ReturnedTaskTooLarge
        );
        let rent_lamports = Rent::get()?.minimum_balance(task_size);
        let lamports = rent_lamports + task_data.crank_reward;
        task_data.rent_amount = rent_lamports;

        system_program::assign(
            CpiContext::new_with_signer(
                self.ctx.accounts.system_program.to_account_info(),
                system_program::Assign {
                    account_to_assign: free_task_account.to_account_info(),
                },
                &[task_seeds!(task_data)],
            ),
            self.ctx.program_id,
        )?;

        free_task_account.realloc(task_size, false)?;

        let task_info = self.ctx.accounts.task.to_account_info();
        let task_remaining_lamports = self
            .ctx
            .accounts
            .task
            .to_account_info()
            .lamports()
            .saturating_sub(self.ctx.accounts.task.crank_reward);
        let lamports_from_task = task_remaining_lamports.min(lamports);
        let lamports_needed_from_queue = lamports.saturating_sub(lamports_from_task);

        if lamports_from_task > 0 {
            task_info.sub_lamports(lamports_from_task)?;
            free_task_account.add_lamports(lamports_from_task)?;
        }

        if lamports_needed_from_queue > 0 {
            self.queue_lamports_needed += lamports_needed_from_queue;
            free_task_account.add_lamports(lamports_needed_from_queue)?;
        }

        let mut data = free_task_account.try_borrow_mut_data()?;
        task_data.try_serialize(&mut data.as_mut())?;

        // The bitmap records the ids this instruction wrote an account for, so an id is marked
        // once the account holds its task and not before.
        self.tasks_to_set.push(task_data.id);

        Ok(())
    }

    fn had_bad_free_task_input(&self) -> bool {
        self.bad_free_task_input
    }

    fn get_tasks_to_set(&self) -> &[u16] {
        &self.tasks_to_set
    }

    fn get_queue_lamports_needed(&self) -> u64 {
        self.queue_lamports_needed
    }
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, RunTaskV0<'info>>,
    args: RunTaskArgsV0,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let task_time = match ctx.accounts.task.trigger {
        TriggerV0::Now => now,
        TriggerV0::Timestamp(timestamp) => timestamp,
    };

    // Use memory-efficient wrapper to avoid deserializing the entire task queue
    let task_queue_account_info = ctx.accounts.task_queue.to_account_info().clone();
    let task_queue_min_lamports = Rent::get()?.minimum_balance(task_queue_account_info.data_len());
    let mut task_queue_data = task_queue_account_info.try_borrow_mut_data()?;
    let mut task_queue = TaskQueueDataWrapper::new(*task_queue_data)?;

    task_queue.header_mut().updated_at = now;

    // A task may spawn no more children than it declared when it was queued. `free_task_ids` is
    // supplied by the crank turner, so the task's own declaration is the authoritative bound.
    require_gte!(
        ctx.accounts.task.free_tasks as usize,
        args.free_task_ids.len(),
        ErrorCode::TooManyReturnedTasks
    );

    // Check for duplicate task IDs
    let mut seen_ids = std::collections::HashSet::new();
    for id in args.free_task_ids.clone() {
        // Strictly less than: id == capacity indexes one byte past the bitmap, which lands on
        // the name length prefix and shifts every offset the wrapper parses after it.
        require_gt!(task_queue.header().capacity, id, ErrorCode::InvalidTaskId);
        // Ensure ID is not already in use in the task queue
        require!(!task_queue.task_exists(id), ErrorCode::TaskIdAlreadyInUse);
        // Check for duplicates in provided IDs
        require!(seen_ids.insert(id), ErrorCode::DuplicateTaskIds);
    }

    let remaining_accounts = ctx.remaining_accounts;

    let transaction = match ctx.accounts.task.transaction.clone() {
        TransactionSourceV0::CompiledV0(compiled_tx) => compiled_tx,
        TransactionSourceV0::RemoteV0 { signer, .. } => {
            let ix_index =
                load_current_index_checked(&ctx.accounts.sysvar_instructions.to_account_info())?;
            // The signature this task is verified against lives in the instruction immediately
            // before this one, so there has to be one. The crank turner composes the transaction
            // and can place this instruction first.
            let verify_ix_index = ix_index
                .checked_sub(1)
                .ok_or_else(|| error!(ErrorCode::MalformedRemoteTransaction))?;
            let ix: Instruction = load_instruction_at_checked(
                verify_ix_index as usize,
                &ctx.accounts.sysvar_instructions,
            )?;
            let data = utils::ed25519::verify_ed25519_ix(&ix, signer.to_bytes().as_slice())?;
            let mut remote_tx = RemoteTaskTransactionV0::try_deserialize(&mut &data[..])?;
            require_eq!(
                remote_tx.transaction.accounts.len(),
                0,
                ErrorCode::MalformedRemoteTransaction
            );

            let num_accounts = remote_tx
                .transaction
                .instructions
                .iter()
                .flat_map(|ix| ix.accounts.iter())
                .chain(
                    remote_tx
                        .transaction
                        .instructions
                        .iter()
                        .map(|ix| &ix.program_id_index),
                )
                .max()
                // A transaction naming no accounts needs none of them. Counted as usize, since
                // the highest index an instruction may name is itself a u8.
                .map_or(0usize, |highest| *highest as usize + 1);

            // The crank turner chooses how many accounts to pass, and the slice below indexes
            // this many of them.
            require_gte!(
                remaining_accounts.len(),
                num_accounts as usize,
                ErrorCode::MismatchedFreeTaskCounts
            );

            let verification_hash = hash(
                &[
                    ctx.accounts.task.key().as_ref(),
                    &ctx.accounts.task.queued_at.to_le_bytes()[..],
                    &remaining_accounts[..num_accounts as usize]
                        .iter()
                        .enumerate()
                        .map(|(i, acc)| {
                            let mut data = Vec::with_capacity(34);
                            data.extend_from_slice(&acc.key.to_bytes());
                            // Summed as usize: three u8 counts can total more than one holds.
                            let writable_end_idx = remote_tx.transaction.num_rw as usize
                                + remote_tx.transaction.num_ro_signers as usize
                                + remote_tx.transaction.num_rw_signers as usize;
                            // The rent refund account may make an account that shouldn't be writable appear writable
                            if i >= writable_end_idx as usize
                                && (*acc.key == ctx.accounts.rent_refund.key()
                                    || *acc.key == ctx.accounts.task_queue.key()
                                    || *acc.key == ctx.accounts.task.key())
                            {
                                data.push(0);
                            } else {
                                data.push(if acc.is_writable { 1 } else { 0 });
                            }
                            data.push(if acc.is_signer { 1 } else { 0 });
                            remote_tx.transaction.accounts.push(*acc.key);
                            data
                        })
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat(),
            );
            require!(
                verification_hash.to_bytes() == remote_tx.verification_hash,
                ErrorCode::InvalidVerificationAccountsHash
            );
            remote_tx.transaction
        }
    };

    // Handle rewards
    let task_fee = ctx.accounts.task.crank_reward;

    let task_info = ctx.accounts.task.to_account_info();
    let crank_turner_info = ctx.accounts.crank_turner.to_account_info();

    task_queue.set_task_exists(ctx.accounts.task.id, false);

    // Save the task queue changes
    task_queue.save()?;

    // Validate that all free task accounts are empty and are valid PDAs
    let free_tasks_start_index = transaction.accounts.len();
    // Validate number of free task accounts matches number of task IDs. Stated as a sum rather
    // than a difference: the crank turner chooses how many accounts to pass, and too few would
    // underflow the subtraction.
    require_eq!(
        ctx.remaining_accounts.len(),
        free_tasks_start_index + args.free_task_ids.len(),
        ErrorCode::MismatchedFreeTaskCounts
    );

    let stale_task_age = task_queue.stale_task_age();
    let min_crank_reward = task_queue.header().min_crank_reward;
    let capacity = task_queue.header().capacity;

    if now.saturating_sub(task_time) <= stale_task_age as i64 {
        task_queue.save()?;
        // We can't hold on to a mutable reference because inner instructions may use the task queue.
        drop(task_queue_data);
        let mut processor = TaskProcessor::new(
            ctx,
            &transaction,
            args.free_task_ids,
            min_crank_reward,
            capacity,
        )?;

        // Validate account keys match
        for (i, account) in transaction.accounts.iter().enumerate() {
            require_eq!(
                account,
                remaining_accounts[i].key,
                ErrorCode::InvalidAccountKey
            );
        }

        // Process each instruction
        for ix in &transaction.instructions {
            processor.process_instruction(ix, remaining_accounts)?;
        }

        // A child that failed on the turner's own id or account choice is not one they may drop.
        require!(
            !processor.had_bad_free_task_input(),
            ErrorCode::InvalidTaskPDA
        );

        // Get the changes we need to make
        let tasks_to_set = processor.get_tasks_to_set().to_vec();
        let queue_lamports_needed = processor.get_queue_lamports_needed();

        drop(processor);
        let task_queue_current_lamports = task_queue_account_info.lamports();
        if queue_lamports_needed > 0 {
            msg!(
                "Need {} lamports from the task queue to fund tasks. Task queue has {} lamports.",
                queue_lamports_needed,
                task_queue_current_lamports
            );
        }
        require_gt!(
            task_queue_current_lamports.saturating_sub(queue_lamports_needed),
            task_queue_min_lamports,
            ErrorCode::TaskQueueInsufficientFunds
        );

        if queue_lamports_needed > 0 {
            task_queue_account_info.sub_lamports(queue_lamports_needed)?;
        }

        let mut task_queue_data = task_queue_account_info.try_borrow_mut_data()?;
        let mut task_queue = TaskQueueDataWrapper::new(*task_queue_data)?;

        // Apply the changes to the task queue
        for task_id in tasks_to_set {
            task_queue.set_task_exists(task_id, true);
        }
    } else {
        msg!(
            "Task is stale with run time {:?}, current time {:?}, closing task",
            task_time,
            now
        );
    }

    msg!("Paying out reward {:?}", task_fee);

    task_info.sub_lamports(task_fee)?;
    crank_turner_info.add_lamports(task_fee)?;

    Ok(())
}
