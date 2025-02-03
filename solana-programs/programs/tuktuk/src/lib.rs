use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub use instructions::*;
pub mod resize_to_fit;
pub mod state;
pub mod utils;

declare_id!("tuktukUrfhXT6ZT77QTU8RQtvgL967uRuVagWF57zVA");

#[program]
pub mod tuktuk {
    use super::*;

    pub fn initialize_tuktuk_config_v0(
        ctx: Context<InitializeTuktukConfigV0>,
        args: InitializeTuktukConfigArgsV0,
    ) -> Result<()> {
        initialize_tuktuk_config_v0::handler(ctx, args)
    }

    pub fn initialize_task_queue_v0(
        ctx: Context<InitializeTaskQueueV0>,
        args: InitializeTaskQueueArgsV0,
    ) -> Result<()> {
        initialize_task_queue_v0::handler(ctx, args)
    }

    pub fn queue_task_v0(ctx: Context<QueueTaskV0>, args: QueueTaskArgsV0) -> Result<()> {
        queue_task_v0::handler(ctx, args)
    }

    pub fn run_task_v0<'info>(
        ctx: Context<'_, '_, '_, 'info, RunTaskV0<'info>>,
        args: RunTaskArgsV0,
    ) -> Result<()> {
        run_task_v0::handler(ctx, args)
    }

    pub fn dequeue_task_v0(ctx: Context<DequeuetaskV0>) -> Result<()> {
        dequeue_task_v0::handler(ctx)
    }

    pub fn close_task_queue_v0(ctx: Context<CloseTaskQueueV0>) -> Result<()> {
        close_task_queue_v0::handler(ctx)
    }

    pub fn dummy_ix(_ctx: Context<DummyIx>) -> Result<()> {
        Err(error!(crate::error::ErrorCode::DummyInstruction))
    }

    pub fn update_task_queue_v0(
        ctx: Context<UpdateTaskQueueV0>,
        args: UpdateTaskQueueArgsV0,
    ) -> Result<()> {
        update_task_queue_v0::handler(ctx, args)
    }
}

#[derive(Accounts)]
pub struct DummyIx<'info> {
    #[account(mut)]
    pub dummy: Account<'info, RemoteTaskTransactionV0>,
}
