use anchor_lang::prelude::*;
use tuktuk_program::{tuktuk::program::Tuktuk, TaskQueueAuthorityV0, TaskQueueV0};

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

/// Requeuing without reading the recorded schedule task can start a second chain beside a live
/// one, which `requeue_cron_task_v1` closes. This version keeps the account list its clients
/// send, since accounts are positional, and refuses so every requeue goes through the gate.
pub fn handler(_ctx: Context<RequeueCronTaskV0>, _args: RequeueCronTaskArgsV0) -> Result<()> {
    Err(error!(ErrorCode::InstructionDeprecated))
}
