//! A task target that hands back a child task through the program-owned tasks-account path,
//! carrying a payload of the caller's chosen size. Tests set the size to exercise what
//! `run_task_v0` does with a returned task across the range of sizes a task account admits.
//!
//! The payload is held in the heap once and serialized once, so the largest size reachable
//! here is bounded by the 32KB an instruction may allocate, not by what a task account holds.

use anchor_lang::{prelude::*, solana_program::instruction::Instruction};
use tuktuk_program::{
    compile_transaction,
    tuktuk::types::TriggerV0,
    write_return_tasks::{write_return_tasks, AccountWithSeeds, PayerInfo, WriteReturnTasksArgs},
    RunTaskReturnV0, TaskReturnV0, TransactionSourceV0,
};

declare_id!("5mwgRJMr7Cnb9jbpphPSBL7QbNKMM8y1TzdhsoZVjztf");

#[program]
pub mod return_example {
    use super::*;

    pub fn return_task_with_payload(
        ctx: Context<ReturnTaskWithPayload>,
        payload_len: u32,
    ) -> Result<RunTaskReturnV0> {
        let (compiled, _) = compile_transaction(
            vec![Instruction {
                program_id: crate::ID,
                accounts: vec![],
                data: vec![0u8; payload_len as usize],
            }],
            vec![],
        )?;

        let task = TaskReturnV0 {
            trigger: TriggerV0::Now,
            transaction: TransactionSourceV0::CompiledV0(compiled),
            crank_reward: None,
            free_tasks: 0,
            description: "payload".to_string(),
        };

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
}

#[derive(Accounts)]
pub struct ReturnTaskWithPayload<'info> {
    /// CHECK: PDA system payer for write_return_tasks
    #[account(mut, seeds = [b"queue_authority"], bump)]
    pub queue_authority: AccountInfo<'info>,
    /// CHECK: written by write_return_tasks
    #[account(mut, seeds = [b"task_return_account"], bump)]
    pub task_return_account: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}
