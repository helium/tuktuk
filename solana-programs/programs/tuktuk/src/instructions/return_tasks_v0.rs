use anchor_lang::prelude::*;

use super::{RunTaskReturnV0, TaskReturnV0};

/// Passthrough: Just returns the tasks passed to it as args.
/// This is useful for remote transactions to schedule themselves.
#[derive(Accounts)]
pub struct ReturnTasksV0 {}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ReturnTasksArgsV0 {
    pub tasks: Vec<TaskReturnV0>,
}

pub fn handler(_ctx: Context<ReturnTasksV0>, _args: ReturnTasksArgsV0) -> Result<RunTaskReturnV0> {
    Ok(RunTaskReturnV0 {
        tasks: _args.tasks,
        tasks_accounts: vec![],
    })
}
