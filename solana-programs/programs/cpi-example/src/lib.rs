use anchor_lang::{
    prelude::*, solana_program::instruction::Instruction as SolanaInstruction, InstructionData,
};
use tuktuk_program::{
    compile_transaction, tuktuk::program::Tuktuk, tuktuk::types::TriggerV0 as TuktukTrigger,
    TaskReturnV0 as TuktukTaskReturn, TransactionSourceV0 as TuktukTransactionSource,
};

declare_id!("cpic9j9sjqvhn2ZX3mqcCgzHKCwiiBTyEszyCwN7MBC");

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ScheduleReturningArgsV0 {
    pub task_id: u16,
    /// Spawn budget the queued task declares.
    pub free_tasks: u8,
    /// Reward carried by the child task it returns.
    pub return_crank_reward: Option<u64>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ReturnTaskArgsV0 {
    pub crank_reward: Option<u64>,
}

/// Builds a terminal child task carrying the caller's chosen reward and spawn budget.
fn noop_task_return(
    system_program: &Program<System>,
    args: &ReturnTaskArgsV0,
) -> Result<TuktukTaskReturn> {
    let (compiled_tx, _) = compile_transaction(
        vec![SolanaInstruction {
            program_id: crate::ID,
            accounts: crate::__cpi_client_accounts_recurring_task::RecurringTask {
                system_program: system_program.to_account_info(),
            }
            .to_account_metas(None)
            .to_vec(),
            data: crate::instruction::NoopTask.data(),
        }],
        vec![],
    )
    .unwrap();

    Ok(TuktukTaskReturn {
        trigger: TuktukTrigger::Now,
        transaction: TuktukTransactionSource::CompiledV0(compiled_tx),
        crank_reward: args.crank_reward,
        // Terminal: the child spawns nothing.
        free_tasks: 0,
        description: "test".to_string(),
    })
}

#[program]
pub mod cpi_example {
    use anchor_lang::{solana_program::instruction::Instruction, InstructionData};
    use tuktuk_program::{
        compile_transaction,
        tuktuk::{
            cpi::{accounts::QueueTaskV0, queue_task_v0},
            types::TriggerV0,
        },
        types::QueueTaskArgsV0,
        write_return_tasks::{
            write_return_tasks, AccountWithSeeds, PayerInfo, WriteReturnTasksArgs,
        },
        RunTaskReturnV0, TaskReturnV0, TransactionSourceV0,
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
                    task_queue_authority: ctx.accounts.task_queue_authority.to_account_info(),
                    task: ctx.accounts.task.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&["queue_authority".as_bytes(), &[ctx.bumps.queue_authority]]],
            ),
            QueueTaskArgsV0 {
                trigger: TriggerV0::Now,
                transaction: TransactionSourceV0::CompiledV0(compiled_tx),
                crank_reward: None,
                free_tasks: 1,
                id: task_id,
                description: "test".to_string(),
            },
        )?;

        Ok(())
    }

    pub fn schedule_with_account_return(
        ctx: Context<ScheduleWithAccountReturn>,
        task_id: u16,
    ) -> Result<()> {
        msg!("Scheduling with a PDA queue authority");
        let (compiled_tx, _) = compile_transaction(
            vec![Instruction {
                program_id: crate::ID,
                accounts:
                    crate::__client_accounts_recurring_task_with_account_return::RecurringTaskWithAccountReturn {
                        system_program: ctx.accounts.system_program.key(),
                        queue_authority: ctx.accounts.queue_authority.key(),
                        task_return_account: ctx.accounts.task_return_account.key(),
                    }
                    .to_account_metas(None)
                    .to_vec(),
                data: crate::instruction::RecurringTaskWithAccountReturn.data(),
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
                    task_queue_authority: ctx.accounts.task_queue_authority.to_account_info(),
                    task: ctx.accounts.task.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&["queue_authority".as_bytes(), &[ctx.bumps.queue_authority]]],
            ),
            QueueTaskArgsV0 {
                trigger: TriggerV0::Now,
                transaction: TransactionSourceV0::CompiledV0(compiled_tx),
                crank_reward: None,
                free_tasks: 15,
                id: task_id,
                description: "test".to_string(),
            },
        )?;

        Ok(())
    }

    /// Queues a task that declares `free_tasks` and, when run, returns a single child task
    /// built from `return_crank_reward` / `return_free_tasks`. Lets a test drive the bounds
    /// a returned task is held to.
    pub fn schedule_returning(ctx: Context<Schedule>, args: ScheduleReturningArgsV0) -> Result<()> {
        let (compiled_tx, _) = compile_transaction(
            vec![Instruction {
                program_id: crate::ID,
                accounts: crate::__cpi_client_accounts_recurring_task::RecurringTask {
                    system_program: ctx.accounts.system_program.to_account_info(),
                }
                .to_account_metas(None)
                .to_vec(),
                data: crate::instruction::ReturnTask {
                    args: ReturnTaskArgsV0 {
                        crank_reward: args.return_crank_reward,
                    },
                }
                .data(),
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
                    task_queue_authority: ctx.accounts.task_queue_authority.to_account_info(),
                    task: ctx.accounts.task.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&["queue_authority".as_bytes(), &[ctx.bumps.queue_authority]]],
            ),
            QueueTaskArgsV0 {
                trigger: TriggerV0::Now,
                transaction: TransactionSourceV0::CompiledV0(compiled_tx),
                crank_reward: None,
                free_tasks: args.free_tasks,
                id: args.task_id,
                description: "test".to_string(),
            },
        )?;

        Ok(())
    }

    /// Returns a single child task with a caller-chosen reward and spawn budget, via return data.
    pub fn return_task(
        ctx: Context<RecurringTask>,
        args: ReturnTaskArgsV0,
    ) -> Result<tuktuk_program::RunTaskReturnV0> {
        Ok(RunTaskReturnV0 {
            tasks: vec![noop_task_return(&ctx.accounts.system_program, &args)?],
            accounts: vec![],
        })
    }

    /// Terminal task: returns nothing, so a test's assertions are not disturbed by further
    /// rescheduling.
    pub fn noop_task(_ctx: Context<RecurringTask>) -> Result<tuktuk_program::RunTaskReturnV0> {
        Ok(RunTaskReturnV0 {
            tasks: vec![],
            accounts: vec![],
        })
    }

    /// As `schedule_returning`, but the child task is handed back in an account rather than in
    /// return data, which is the other path that creates returned tasks.
    pub fn schedule_returning_via_account(
        ctx: Context<ScheduleWithAccountReturn>,
        args: ScheduleReturningArgsV0,
    ) -> Result<()> {
        let (compiled_tx, _) = compile_transaction(
            vec![Instruction {
                program_id: crate::ID,
                accounts:
                    crate::__client_accounts_recurring_task_with_account_return::RecurringTaskWithAccountReturn {
                        system_program: ctx.accounts.system_program.key(),
                        queue_authority: ctx.accounts.queue_authority.key(),
                        task_return_account: ctx.accounts.task_return_account.key(),
                    }
                    .to_account_metas(None)
                    .to_vec(),
                data: crate::instruction::ReturnTaskViaAccount {
                    args: ReturnTaskArgsV0 {
                        crank_reward: args.return_crank_reward,
                    },
                }
                .data(),
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
                    task_queue_authority: ctx.accounts.task_queue_authority.to_account_info(),
                    task: ctx.accounts.task.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                &[&["queue_authority".as_bytes(), &[ctx.bumps.queue_authority]]],
            ),
            QueueTaskArgsV0 {
                trigger: TriggerV0::Now,
                transaction: TransactionSourceV0::CompiledV0(compiled_tx),
                crank_reward: None,
                free_tasks: args.free_tasks,
                id: args.task_id,
                description: "test".to_string(),
            },
        )?;

        Ok(())
    }

    /// Returns a single child task with a caller-chosen reward and spawn budget, via an account.
    pub fn return_task_via_account(
        ctx: Context<RecurringTaskWithAccountReturn>,
        args: ReturnTaskArgsV0,
    ) -> Result<tuktuk_program::RunTaskReturnV0> {
        let task = noop_task_return(&ctx.accounts.system_program, &args)?;
        let return_accounts = write_return_tasks(WriteReturnTasksArgs {
            program_id: crate::ID,
            payer_info: PayerInfo::SystemPayer {
                account_info: ctx.accounts.queue_authority.to_account_info(),
                seeds: vec![b"queue_authority".to_vec(), vec![ctx.bumps.queue_authority]],
            },
            accounts: vec![AccountWithSeeds {
                account: ctx.accounts.task_return_account.to_account_info(),
                seeds: vec![
                    b"task_return_account".to_vec(),
                    vec![ctx.bumps.task_return_account],
                ],
            }],
            system_program: ctx.accounts.system_program.to_account_info(),
            tasks: std::iter::once(task),
        })?
        .used_accounts;

        Ok(RunTaskReturnV0 {
            tasks: vec![],
            accounts: return_accounts,
        })
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
                transaction: TransactionSourceV0::CompiledV0(compiled_tx),
                crank_reward: None,
                free_tasks: 1,
                description: "test".to_string(),
            }],
            accounts: vec![],
        })
    }

    // An example of using an account for larger return values
    pub fn recurring_task_with_account_return(
        ctx: Context<RecurringTaskWithAccountReturn>,
    ) -> Result<tuktuk_program::RunTaskReturnV0> {
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

        msg!("Rescheduling task via return account");
        let return_accounts = write_return_tasks(WriteReturnTasksArgs {
            program_id: crate::ID,
            payer_info: PayerInfo::SystemPayer {
                account_info: ctx.accounts.queue_authority.to_account_info(),
                seeds: vec![b"queue_authority".to_vec(), vec![ctx.bumps.queue_authority]],
            },
            accounts: vec![AccountWithSeeds {
                account: ctx.accounts.task_return_account.to_account_info(),
                seeds: vec![
                    b"task_return_account".to_vec(),
                    vec![ctx.bumps.task_return_account],
                ],
            }],
            system_program: ctx.accounts.system_program.to_account_info(),
            tasks: (0..15).map(|_| TaskReturnV0 {
                trigger: TriggerV0::Now,
                transaction: TransactionSourceV0::CompiledV0(compiled_tx.clone()),
                crank_reward: None,
                free_tasks: 1,
                description: "test".to_string(),
            }),
        })?
        .used_accounts;
        Ok(RunTaskReturnV0 {
            tasks: vec![],
            accounts: return_accounts,
        })
    }
}

#[derive(Accounts)]
pub struct RecurringTask<'info> {
    // This is a dummy account to show how to pass accounts to scheduling.
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RecurringTaskWithAccountReturn<'info> {
    /// CHECK: Via seeds
    #[account(
        mut,
        seeds = [b"queue_authority"],
        bump
    )]
    pub queue_authority: AccountInfo<'info>,
    // This is a dummy account to show how to pass accounts to scheduling.
    pub system_program: Program<'info, System>,
    /// CHECK: Used to write return data
    #[account(
        mut,
        seeds = [b"task_return_account"],
        bump
    )]
    pub task_return_account: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct Schedule<'info> {
    #[account(mut)]
    /// CHECK: Don't need to parse this account, just using it in CPI
    pub task_queue: UncheckedAccount<'info>,
    /// CHECK: Don't need to parse this account, just using it in CPI
    pub task_queue_authority: UncheckedAccount<'info>,
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

#[derive(Accounts)]
pub struct ScheduleWithAccountReturn<'info> {
    #[account(mut)]
    /// CHECK: Don't need to parse this account, just using it in CPI
    pub task_queue: UncheckedAccount<'info>,
    /// CHECK: Don't need to parse this account, just using it in CPI
    pub task_queue_authority: UncheckedAccount<'info>,
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
    /// CHECK: Used to write return data
    #[account(
        seeds = [b"task_return_account"],
        bump
    )]
    pub task_return_account: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub tuktuk_program: Program<'info, Tuktuk>,
}
