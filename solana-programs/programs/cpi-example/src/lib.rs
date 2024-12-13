use anchor_lang::{prelude::*, solana_program::instruction::Instruction, InstructionData};
use tuktuk_program::{tuktuk::program::Tuktuk, TaskQueueV0};

declare_id!("cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC");

#[program]
pub mod cpi_example {
    use tuktuk_program::{
        compile_transaction,
        tuktuk::{
            cpi::{accounts::QueueTaskV0, queue_task_v0},
            types::TriggerV0,
        },
        types::QueueTaskArgsV0,
        RunTaskReturnV0, TaskReturnV0,
    };

    use super::*;

    pub fn schedule(ctx: Context<Schedule>, task_id: u16) -> Result<()> {
        msg!("Scheduling with a PDA queue authority");
        let (compiled_tx, _) = compile_transaction(
            vec![Instruction {
                program_id: crate::ID,
                accounts: crate::__cpi_client_accounts_recurring_task::RecurringTask {
                    system_program: ctx.accounts.system_program.to_account_info(),
                }
                .to_account_metas(None)
                .to_vec(),
                data: crate::instruction::RecurringTask.data(),
            }],
            vec![],
        )
        .unwrap();

        queue_task_v0(
            CpiContext::new_with_signer(
                ctx.accounts.tuktuk_program.to_account_info(),
                QueueTaskV0 {
                    payer: ctx.accounts.queue_authority.to_account_info(),
                    queue_authority: ctx.accounts.queue_authority.to_account_info(),
                    task_queue: ctx.accounts.task_queue.to_account_info(),
                    task: ctx.accounts.task.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&["queue_authority".as_bytes(), &[ctx.bumps.queue_authority]]],
            ),
            QueueTaskArgsV0 {
                trigger: TriggerV0::Now,
                transaction: compiled_tx,
                crank_reward: None,
                free_tasks: 1,
                id: task_id,
            },
        )?;

        Ok(())
    }

    pub fn recurring_task(ctx: Context<RecurringTask>) -> Result<tuktuk_program::RunTaskReturnV0> {
        msg!("Running recurring task!");
        let (compiled_tx, _) = compile_transaction(
            vec![Instruction {
                program_id: crate::ID,
                accounts: crate::__cpi_client_accounts_recurring_task::RecurringTask {
                    system_program: ctx.accounts.system_program.to_account_info(),
                }
                .to_account_metas(None)
                .to_vec(),
                data: crate::instruction::RecurringTask.data(),
            }],
            vec![],
        )
        .unwrap();

        msg!("Rescheduling task via return value");
        Ok(RunTaskReturnV0 {
            tasks: vec![TaskReturnV0 {
                trigger: TriggerV0::Timestamp(Clock::get()?.unix_timestamp + 1),
                transaction: compiled_tx,
                crank_reward: None,
                free_tasks: 1,
            }],
        })
    }
}

#[derive(Accounts)]
pub struct RecurringTask<'info> {
    // This is a dummy account to show how to pass accounts to scheduling.
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Schedule<'info> {
    #[account(mut)]
    /// CHECK: Don't need to parse this account, just using it in CPI
    pub task_queue: UncheckedAccount<'info>,
    /// CHECK: Initialized in CPI
    #[account(mut)]
    pub task: AccountInfo<'info>,
    /// CHECK: Via seeds
    #[account(
        mut,
        seeds = [b"queue_authority"],
        bump
    )]
    pub queue_authority: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub tuktuk_program: Program<'info, Tuktuk>,
}
