use anchor_lang::prelude::*;

use super::{RunTaskReturnV0, TaskReturnV0};

/// Passthrough: Just returns the tasks passed to it as args.
/// This is useful for remote transactions to schedule themselves.
#[derive(Accounts)]
pub struct ReturnTasksV0<'info> {
    pub system_program: Program<'info, System>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ReturnTasksArgsV0 {
    pub tasks: Vec<TaskReturnV0>,
}

pub fn handler(_ctx: Context<ReturnTasksV0>, args: ReturnTasksArgsV0) -> Result<RunTaskReturnV0> {
    Ok(RunTaskReturnV0 {
        tasks: args.tasks,
        tasks_accounts: vec![],
    })
}
