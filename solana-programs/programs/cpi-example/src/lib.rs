use anchor_lang::prelude::*;
use tuktuk_sdk::tuktuk::{tuktuk::program::Tuktuk, TaskQueueV0};

declare_id!("cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC");

#[program]
pub mod cpi_example {
    use anchor_lang::{solana_program::instruction::Instruction, InstructionData};
    use tuktuk_sdk::{
        compiled_transaction::*,
        tuktuk::{
            tuktuk::cpi::{accounts::QueueTaskV0, queue_task_v0},
            types::{QueueTaskArgsV0, TriggerV0},
        },
    };

    use super::*;

    pub fn schedule_next(ctx: Context<ScheduleNext>) -> Result<()> {
        let my_tx = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            crate::__cpi_client_accounts_schedule_next::ScheduleNext {
                queue_authority: ctx.accounts.queue_authority.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                tuktuk_program: ctx.accounts.tuktuk_program.to_account_info(),
                task_queue: ctx.accounts.task_queue.to_account_info(),
                free_task_1: ctx.accounts.free_task_1.to_account_info(),
            },
        );
        // Only take the first 3 accounts. Tuktuk will pass the task queue and a free task account automatically.
        // We do this because we don't actually know what the task ID will be since its PDA depends on the available task ID.
        let account_metas = my_tx.accounts.to_account_metas(None)[..3].to_vec();
        let data = crate::instruction::ScheduleNext.data();
        let (compiled_tx, _) = compile_transaction(
            vec![Instruction {
                program_id: crate::ID,
                accounts: account_metas,
                data,
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
                    task: ctx.accounts.free_task_1.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&[b"queue_authority".as_ref(), &[ctx.bumps.queue_authority]]],
            )
            .with_remaining_accounts(vec![
                ctx.accounts.queue_authority.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.tuktuk_program.to_account_info(),
            ]),
            QueueTaskArgsV0 {
                id: ctx.accounts.task_queue.next_available_task_id().unwrap() as u16,
                // 5 seconds from now
                trigger: TriggerV0::Timestamp(Clock::get()?.unix_timestamp + 5),
                transaction: compiled_tx.into(),
                crank_reward: None,
                free_tasks: 1,
            },
        )?;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct ScheduleNext<'info> {
    #[account(
        mut,
        seeds = [b"queue_authority".as_ref()],
        bump,
    )]
    pub queue_authority: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub tuktuk_program: Program<'info, Tuktuk>,
    pub task_queue: Account<'info, TaskQueueV0>,
    /// CHECK: free task account
    pub free_task_1: UncheckedAccount<'info>,
}
