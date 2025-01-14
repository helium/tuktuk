use anchor_lang::prelude::*;
use tuktuk_program::RunTaskReturnV0;

pub mod error;
pub mod instructions;
pub use instructions::*;
mod resize_to_fit;
pub mod state;

declare_id!("cronAjRZnJn3MTP3B9kE62NWDrjSuAPVXf9c4hu4grM");

#[program]
pub mod cron {

    use super::*;

    pub fn initialize_cron_job_v0(
        ctx: Context<InitializeCronJobV0>,
        args: InitializeCronJobArgsV0,
    ) -> Result<()> {
        initialize_cron_job_v0::handler(ctx, args)
    }

    pub fn add_cron_transaction_v0(
        ctx: Context<AddCronTransactionV0>,
        args: AddCronTransactionArgsV0,
    ) -> Result<()> {
        add_cron_transaction_v0::handler(ctx, args)
    }

    pub fn queue_cron_tasks_v0(ctx: Context<QueueCronTasksV0>) -> Result<RunTaskReturnV0> {
        queue_cron_tasks_v0::handler(ctx)
    }

    pub fn remove_cron_transaction_v0(
        ctx: Context<RemoveCronTransactionV0>,
        args: RemoveCronTransactionArgsV0,
    ) -> Result<()> {
        remove_cron_transaction_v0::handler(ctx, args)
    }

    pub fn close_cron_job_v0(ctx: Context<CloseCronJobV0>) -> Result<()> {
        close_cron_job_v0::handler(ctx)
    }
}
