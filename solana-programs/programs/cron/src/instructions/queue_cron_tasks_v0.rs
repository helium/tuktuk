use std::str::FromStr;

use anchor_lang::{prelude::*, solana_program::instruction::Instruction, InstructionData};
use chrono::{DateTime, Utc};
use clockwork_cron::Schedule;
use tuktuk_program::{
    compile_transaction,
    write_return_tasks::{
        write_return_tasks, AccountWithSeeds, PayerInfo, WriteReturnTasksArgs,
        WriteReturnTasksReturn,
    },
    RunTaskReturnV0, TaskQueueV0, TaskReturnV0, TransactionSourceV0, TriggerV0,
};

use crate::state::{CronJobTransactionV0, CronJobV0};

// Queue tasks 5 minutes before the cron job is scheduled to run
// This way we don't take up space in the task queue for tasks that are running
// A long time from now
pub const QUEUE_TASK_DELAY: i64 = 60 * 5;

#[derive(Accounts)]
pub struct QueueCronTasksV0<'info> {
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
}

#[macro_export]
macro_rules! try_from {
    ($ty: ty, $acc: expr) => {{
        let account_info = $acc.as_ref();
        <$ty>::try_from(unsafe {
            core::mem::transmute::<
                &anchor_lang::prelude::AccountInfo<'_>,
                &anchor_lang::prelude::AccountInfo<'_>,
            >(account_info)
        })
    }};
}

pub fn handler(ctx: Context<QueueCronTasksV0>) -> Result<RunTaskReturnV0> {
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
    require_eq!(cron_job.task_queue, ctx.accounts.task_queue.key());

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

    if now - cron_job.current_exec_ts > stale_task_age as i64 {
        msg!("Cron job is stale, resetting");
        cron_job.current_exec_ts = now;
        cron_job.current_transaction_id = 0;
    }

    let max_num_tasks_remaining = cron_job.next_transaction_id - cron_job.current_transaction_id;
    let num_tasks_to_queue =
        (cron_job.num_tasks_per_queue_call as u32).min(max_num_tasks_remaining);
    cron_job.current_transaction_id += num_tasks_to_queue;

    let trigger = TriggerV0::Timestamp(cron_job.current_exec_ts);

    // If we reached the end this time, reset to 0 and move the next execution time forward
    if cron_job.current_transaction_id == cron_job.next_transaction_id {
        cron_job.current_transaction_id = 0;
        let schedule = Schedule::from_str(&cron_job.schedule).unwrap();
        // Find the next execution time after the last one
        let ts = cron_job.current_exec_ts;
        let date_time_ts = &DateTime::<Utc>::from_naive_utc_and_offset(
            DateTime::from_timestamp(ts, 0).unwrap().naive_utc(),
            Utc,
        );
        cron_job.current_exec_ts = schedule.next_after(date_time_ts).unwrap().timestamp();
        msg!(
            "Will have finished execution ts: {}, moving to {}",
            ts,
            cron_job.current_exec_ts
        );
    }

    let remaining_accounts = (cron_job.current_transaction_id
        ..cron_job.current_transaction_id + cron_job.num_tasks_per_queue_call as u32)
        .map(|i| {
            Pubkey::find_program_address(
                &[
                    b"cron_job_transaction",
                    cron_job.key().as_ref(),
                    &i.to_le_bytes(),
                ],
                &crate::ID,
            )
            .0
        })
        .collect::<Vec<Pubkey>>();

    let (queue_tx, _) = compile_transaction(
        vec![Instruction {
            program_id: crate::ID,
            accounts: [
                crate::__cpi_client_accounts_queue_cron_tasks_v0::QueueCronTasksV0 {
                    cron_job: cron_job.to_account_info(),
                    task_queue: ctx.accounts.task_queue.to_account_info(),
                    task_return_account_1: ctx.accounts.task_return_account_1.to_account_info(),
                    task_return_account_2: ctx.accounts.task_return_account_2.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                }
                .to_account_metas(None),
                remaining_accounts
                    .iter()
                    .map(|pubkey| AccountMeta::new_readonly(*pubkey, false))
                    .collect::<Vec<AccountMeta>>(),
            ]
            .concat(),
            data: crate::instruction::QueueCronTasksV0.data(),
        }],
        vec![],
    )?;
    let free_tasks_per_transaction = cron_job.free_tasks_per_transaction;
    let trunc_name = cron_job.name.chars().take(32).collect::<String>();
    let tasks = std::iter::once(TaskReturnV0 {
        trigger: TriggerV0::Timestamp(cron_job.current_exec_ts - QUEUE_TASK_DELAY),
        transaction: TransactionSourceV0::CompiledV0(queue_tx),
        crank_reward: None,
        free_tasks: cron_job.num_tasks_per_queue_call + 1,
        description: format!("queue {}", trunc_name),
    })
    .chain((0..num_tasks_to_queue as usize).filter_map(|i| {
        let transaction = ctx.remaining_accounts[i].clone();
        if transaction.data_is_empty() {
            return None;
        }

        let parsed_transaction: CronJobTransactionV0 =
            AccountDeserialize::try_deserialize(&mut &transaction.data.borrow()[..]).ok()?;

        Some(TaskReturnV0 {
            trigger,
            transaction: parsed_transaction.transaction,
            crank_reward: None,
            free_tasks: free_tasks_per_transaction,
            description: format!("{} {}", trunc_name, parsed_transaction.id),
        })
    }));

    // Past all the CronJobTransaction are the free tasks
    cron_job.next_schedule_task =
        ctx.remaining_accounts[cron_job.num_tasks_per_queue_call as usize].key();

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
                msg!(
                    "Not enough lamports to fund tasks. Please requeue cron job when you have enough lamports. {}",
                    cron_job_info.lamports()
                );
                cron_job.removed_from_queue = true;
                cron_job.next_schedule_task = Pubkey::default();
                cron_job.exit(&crate::ID)?;
                Ok(RunTaskReturnV0 {
                    tasks: vec![],
                    accounts: vec![],
                })
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
        Err(e) if e.to_string().contains("rent exempt") => {
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
        Err(e) => Err(e),
    }
}
