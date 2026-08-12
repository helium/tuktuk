use anchor_lang::prelude::*;
use tuktuk_program::{
    write_return_tasks::{
        write_return_tasks, AccountWithSeeds, PayerInfo, WriteReturnTasksArgs,
        WriteReturnTasksReturn,
    },
    RunTaskReturnV0, TaskQueueV0, TaskReturnV0, TransactionSourceV0, TriggerV0,
};

use crate::{
    error::ErrorCode,
    schedule::{
        compile_schedule_transaction, effective_tasks_per_queue_call, next_exec_ts,
        running_schedule_task, trunc_name, QUEUE_TASK_DELAY,
    },
    state::{CronJobTransactionV0, CronJobV0},
    try_from,
};

#[derive(Accounts)]
pub struct QueueCronTasksV1<'info> {
    /// CHECK: Checked via require in handler
    #[account(mut)]
    pub cron_job: UncheckedAccount<'info>,
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    /// CHECK: Used to write return data
    #[account(
        mut,
        seeds = [b"task_return_account_1", cron_job.key().as_ref()],
        bump
    )]
    pub task_return_account_1: AccountInfo<'info>,
    /// CHECK: Used to write return data
    #[account(
        mut,
        seeds = [b"task_return_account_2", cron_job.key().as_ref()],
        bump
    )]
    pub task_return_account_2: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    /// The PDA tuktuk signs for when it runs a task whose transaction declared this cron job's
    /// seeds. Which task of the job's schedule chain the run is, is settled by the
    /// `recorded_schedule_task` checks in the handler.
    #[account(
        seeds = [b"custom", task_queue.key().as_ref(), b"cron", cron_job.key().as_ref()],
        seeds::program = tuktuk_program::tuktuk::ID,
        bump,
    )]
    pub cron_signer: Signer<'info>,
    /// CHECK: Address checked. Names the task this instruction is running under.
    #[account(address = anchor_lang::solana_program::sysvar::instructions::ID)]
    pub sysvar_instructions: AccountInfo<'info>,
    /// CHECK: Checked against `cron_job.next_schedule_task` in the handler, which is where the
    /// cron job records the one task carrying its schedule.
    pub recorded_schedule_task: AccountInfo<'info>,
}

pub fn handler(ctx: Context<QueueCronTasksV1>) -> Result<RunTaskReturnV0> {
    let stale_task_age = ctx.accounts.task_queue.stale_task_age;
    let now = Clock::get()?.unix_timestamp;
    if ctx.accounts.cron_job.data_is_empty() {
        msg!("Cron job was closed, completing task");
        return Ok(RunTaskReturnV0 {
            tasks: vec![],
            accounts: vec![],
        });
    }
    let mut cron_job = try_from!(Account<CronJobV0>, &ctx.accounts.cron_job)?;
    // The signer above is derived from the passed task queue, so this is what ties it to the
    // queue the cron job actually runs on.
    require_eq!(cron_job.task_queue, ctx.accounts.task_queue.key());

    // A cron job carries one schedule chain, and records which task holds it. A run is that
    // task, or it adopts a record that holds nothing, which is the state a chain left behind
    // when it ended, and the state `requeue_cron_task_v1` also answers to.
    require_keys_eq!(
        ctx.accounts.recorded_schedule_task.key(),
        cron_job.next_schedule_task,
        ErrorCode::WrongScheduleTask
    );
    // Only a live record has to be matched against the running task, so adopting an ended chain
    // does not depend on the run being one this program can identify. Reading the running task
    // needs `run_task_v0` to be the top-level instruction, which a caller that reaches it through
    // its own CPI does not give.
    if !ctx.accounts.recorded_schedule_task.data_is_empty() {
        require!(
            running_schedule_task(&ctx.accounts.sysvar_instructions)?
                == cron_job.next_schedule_task,
            ErrorCode::WrongScheduleTask
        );
    }

    // Only proceed if we're within the queue window of the next execution
    if (now + QUEUE_TASK_DELAY) < cron_job.current_exec_ts {
        msg!("Too early to queue tasks, current time {} is not within {} seconds of next execution {}",
            now, QUEUE_TASK_DELAY, cron_job.current_exec_ts);

        // Return Ok so this task closes
        return Ok(RunTaskReturnV0 {
            tasks: vec![],
            accounts: vec![],
        });
    }

    // The records the running task names were derived from this field when that task was compiled,
    // and only this instruction and a requeue write it, so the value here is the one it was built
    // around. Read before the reset below, which moves the cycle back to the first record while
    // leaving the task's own account list where it is.
    let expected_first_transaction_id = cron_job.current_transaction_id;

    let reset = now - cron_job.current_exec_ts > stale_task_age as i64;
    if reset {
        msg!("Cron job is stale, resetting");
        cron_job.current_exec_ts = now;
        cron_job.current_transaction_id = 0;
    }

    // The `cron_job_transaction` records this task names, then the free task accounts. The crank
    // turner chooses how many free tasks to pass, so the slice length is not fixed by the
    // program; the first free task is where the next schedule task will be created.
    let num_tasks_per_queue_call = effective_tasks_per_queue_call(&cron_job) as usize;
    let accounts = ctx
        .remaining_accounts
        .get(..=num_tasks_per_queue_call)
        .ok_or_else(|| error!(ErrorCode::NotEnoughAccounts))?;
    let next_schedule_task = accounts[num_tasks_per_queue_call].key();

    let max_num_tasks_remaining = cron_job
        .next_transaction_id
        .saturating_sub(cron_job.current_transaction_id);
    // The account list above was compiled around `expected_first_transaction_id`, so a reset that
    // moved the cycle back off it leaves the list naming records this run may not queue. That
    // beat only re-arms: the successor is compiled from the reset state, and its trigger is
    // already in the past, so it carries the execution straight away.
    let num_tasks_to_queue = if reset && expected_first_transaction_id != 0 {
        0
    } else {
        (num_tasks_per_queue_call as u32).min(max_num_tasks_remaining)
    };
    cron_job.current_transaction_id += num_tasks_to_queue;

    // The records to queue are named by the task's stored transaction, and only their contents say
    // which cron job they belong to and which index they hold. Each must be this job's, at the
    // index the task was compiled around, so a run queues the job's own records once each and in
    // order however the cycle moved. Read once and kept: the allocator never hands a record back,
    // so reaching for the same bytes again would cost them twice.
    let mut records = Vec::with_capacity(num_tasks_to_queue as usize);
    for (i, account) in accounts
        .iter()
        .take(num_tasks_to_queue as usize)
        .enumerate()
    {
        // What a removed record leaves behind.
        if account.data_is_empty() {
            continue;
        }
        require_keys_eq!(*account.owner, crate::ID, ErrorCode::WrongCronTransaction);
        let record: CronJobTransactionV0 =
            AccountDeserialize::try_deserialize(&mut &account.data.borrow()[..])
                .map_err(|_| error!(ErrorCode::WrongCronTransaction))?;
        require_keys_eq!(
            record.cron_job,
            cron_job.key(),
            ErrorCode::WrongCronTransaction
        );
        require_eq!(
            record.id,
            expected_first_transaction_id + i as u32,
            ErrorCode::WrongCronTransaction
        );
        records.push(record);
    }

    let trigger = TriggerV0::Timestamp(cron_job.current_exec_ts);

    // If we reached the end this time, reset to 0 and move the next execution time forward
    if cron_job.current_transaction_id == cron_job.next_transaction_id {
        cron_job.current_transaction_id = 0;
        let ts = cron_job.current_exec_ts;
        cron_job.current_exec_ts = next_exec_ts(&cron_job.schedule, ts)?;
        msg!(
            "Will have finished execution ts: {}, moving to {}",
            ts,
            cron_job.current_exec_ts
        );
    }

    let queue_tx = compile_schedule_transaction(
        &cron_job,
        cron_job.key(),
        ctx.accounts.task_return_account_1.key(),
        ctx.accounts.task_return_account_2.key(),
        next_schedule_task,
    )?;
    let free_tasks_per_transaction = cron_job.free_tasks_per_transaction;
    let trunc_name = trunc_name(&cron_job.name);
    // The schedule task is written first, so it is the one created in `next_schedule_task`.
    let tasks = std::iter::once(TaskReturnV0 {
        trigger: TriggerV0::Timestamp(cron_job.current_exec_ts - QUEUE_TASK_DELAY),
        transaction: TransactionSourceV0::CompiledV0(queue_tx),
        crank_reward: None,
        free_tasks: num_tasks_per_queue_call as u8 + 1,
        description: format!("queue {}", trunc_name),
    })
    .chain(records.into_iter().map(|record| TaskReturnV0 {
        trigger,
        description: format!("{} {}", trunc_name, record.id),
        transaction: record.transaction,
        crank_reward: None,
        free_tasks: free_tasks_per_transaction,
    }));

    cron_job.next_schedule_task = next_schedule_task;

    let res = write_return_tasks(WriteReturnTasksArgs {
        program_id: crate::ID,
        payer_info: PayerInfo::PdaPayer(cron_job.to_account_info()),
        accounts: vec![
            AccountWithSeeds {
                account: ctx.accounts.task_return_account_1.to_account_info(),
                seeds: vec![
                    b"task_return_account_1".to_vec(),
                    cron_job.key().as_ref().to_vec(),
                    vec![ctx.bumps.task_return_account_1],
                ],
            },
            AccountWithSeeds {
                account: ctx.accounts.task_return_account_2.to_account_info(),
                seeds: vec![
                    b"task_return_account_2".to_vec(),
                    cron_job.key().as_ref().to_vec(),
                    vec![ctx.bumps.task_return_account_2],
                ],
            },
        ],
        system_program: ctx.accounts.system_program.to_account_info(),
        tasks,
    });

    match res {
        Ok(WriteReturnTasksReturn {
            used_accounts,
            total_tasks,
        }) => {
            msg!("Queued {} tasks", total_tasks);

            // Transfer needed lamports from the cron job to the task queue
            let cron_job_info = cron_job.to_account_info();
            let cron_job_min_lamports = Rent::get()?.minimum_balance(cron_job_info.data_len());
            let lamports = ctx.accounts.task_queue.min_crank_reward * total_tasks as u64;
            if cron_job_info.lamports() < cron_job_min_lamports + lamports {
                stand_down(&mut cron_job)
            } else {
                cron_job.removed_from_queue = false;
                cron_job_info.sub_lamports(lamports)?;
                ctx.accounts
                    .task_queue
                    .to_account_info()
                    .add_lamports(lamports)?;

                cron_job.exit(&crate::ID)?;
                Ok(RunTaskReturnV0 {
                    tasks: vec![],
                    accounts: used_accounts,
                })
            }
        }
        // `write_return_tasks` refuses for the same reason: the cron job cannot cover the rent
        // of the accounts the return is written into without dropping below its own.
        Err(Error::AnchorError(e))
            if e.error_code_number
                == anchor_lang::error::ErrorCode::ConstraintRentExempt as u32 =>
        {
            stand_down(&mut cron_job)
        }
        Err(e) => Err(e),
    }
}

/// The cron job cannot fund what this run would queue, so it leaves the queue and waits for a
/// requeue. Clearing `next_schedule_task` is what lets the requeue adopt the chain.
fn stand_down(cron_job: &mut Account<CronJobV0>) -> Result<RunTaskReturnV0> {
    msg!(
        "Not enough lamports to fund tasks. Please requeue cron job when you have enough lamports. {}",
        cron_job.to_account_info().lamports()
    );
    cron_job.removed_from_queue = true;
    cron_job.next_schedule_task = Pubkey::default();
    cron_job.exit(&crate::ID)?;

    Ok(RunTaskReturnV0 {
        tasks: vec![],
        accounts: vec![],
    })
}
