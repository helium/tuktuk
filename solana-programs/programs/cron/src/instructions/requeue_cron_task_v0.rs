use std::str::FromStr;

use anchor_lang::{prelude::*, solana_program::instruction::Instruction, InstructionData};
use chrono::{DateTime, Utc};
use clockwork_cron::Schedule;
use tuktuk_program::{
    compile_transaction,
    tuktuk::{
        cpi::{accounts::QueueTaskV0, queue_task_v0},
        program::Tuktuk,
    },
    types::QueueTaskArgsV0,
    TaskQueueAuthorityV0, TaskQueueV0, TransactionSourceV0, TriggerV0,
};

use super::QUEUE_TASK_DELAY;
use crate::{error::ErrorCode, state::CronJobV0};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RequeueCronTaskArgsV0 {
    pub task_id: u16,
}

#[derive(Accounts)]
#[instruction(args: RequeueCronTaskArgsV0)]
pub struct RequeueCronTaskV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub authority: Signer<'info>,
    pub queue_authority: Signer<'info>,
    #[account(
        seeds = [b"task_queue_authority", task_queue.key().as_ref(), queue_authority.key().as_ref()],
        bump = task_queue_authority.bump_seed,
        seeds::program = tuktuk_program.key(),
    )]
    pub task_queue_authority: Box<Account<'info, TaskQueueAuthorityV0>>,
    #[account(
        mut,
        has_one = authority,
        constraint = cron_job.removed_from_queue || cron_job.next_schedule_task == Pubkey::default()
    )]
    pub cron_job: Box<Account<'info, CronJobV0>>,
    #[account(mut)]
    pub task_queue: Box<Account<'info, TaskQueueV0>>,
    /// CHECK: Initialized in CPI
    #[account(mut)]
    pub task: AccountInfo<'info>,
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
    pub tuktuk_program: Program<'info, Tuktuk>,
}

pub fn handler(ctx: Context<RequeueCronTaskV0>, args: RequeueCronTaskArgsV0) -> Result<()> {
    let schedule = Schedule::from_str(&ctx.accounts.cron_job.schedule);
    if let Err(e) = schedule {
        msg!("Invalid schedule: {}", e);
        return Err(error!(ErrorCode::InvalidSchedule));
    }

    let ts = Clock::get().unwrap().unix_timestamp;
    let now = &DateTime::<Utc>::from_naive_utc_and_offset(
        DateTime::from_timestamp(ts, 0).unwrap().naive_utc(),
        Utc,
    );

    ctx.accounts.cron_job.next_schedule_task = ctx.accounts.task.key();
    ctx.accounts.cron_job.removed_from_queue = false;
    ctx.accounts.cron_job.current_exec_ts = schedule.unwrap().next_after(now).unwrap().timestamp();

    let remaining_accounts = (ctx.accounts.cron_job.current_transaction_id
        ..ctx.accounts.cron_job.current_transaction_id
            + ctx.accounts.cron_job.num_tasks_per_queue_call as u32)
        .map(|i| {
            Pubkey::find_program_address(
                &[
                    b"cron_job_transaction",
                    ctx.accounts.cron_job.key().as_ref(),
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
                    cron_job: ctx.accounts.cron_job.to_account_info(),
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

    let trunc_name = ctx
        .accounts
        .cron_job
        .name
        .chars()
        .take(32)
        .collect::<String>();
    queue_task_v0(
        CpiContext::new(
            ctx.accounts.tuktuk_program.to_account_info(),
            QueueTaskV0 {
                payer: ctx.accounts.payer.to_account_info(),
                queue_authority: ctx.accounts.queue_authority.to_account_info(),
                task_queue_authority: ctx.accounts.task_queue_authority.to_account_info(),
                task_queue: ctx.accounts.task_queue.to_account_info(),
                task: ctx.accounts.task.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
        ),
        QueueTaskArgsV0 {
            trigger: TriggerV0::Timestamp(ctx.accounts.cron_job.current_exec_ts - QUEUE_TASK_DELAY),
            transaction: TransactionSourceV0::CompiledV0(queue_tx),
            crank_reward: None,
            free_tasks: ctx.accounts.cron_job.num_tasks_per_queue_call + 1,
            id: args.task_id,
            description: format!("queue {}", trunc_name),
        },
    )?;

    Ok(())
}
