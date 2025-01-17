use anchor_lang::prelude::*;
use tuktuk_program::{CompiledTransactionV0, TransactionSourceV0};

#[account]
#[derive(Default, InitSpace)]
pub struct UserCronJobsV0 {
    pub authority: Pubkey,
    pub min_cron_job_id: u32,
    pub next_cron_job_id: u32,
    pub bump_seed: u8,
}

#[account]
#[derive(Default)]
pub struct CronJobV0 {
    pub id: u32,
    pub user_cron_jobs: Pubkey,
    pub task_queue: Pubkey,
    pub authority: Pubkey,
    pub free_tasks_per_transaction: u8,
    pub num_tasks_per_queue_call: u8,
    pub schedule: String,
    pub name: String,
    pub current_exec_ts: i64,
    pub current_transaction_id: u32,
    pub num_transactions: u32,
    pub next_transaction_id: u32,
    // A cron job is removed from the queue when it no longer has enough lamports to fund tasks
    // Once this is set, you need to requeue the cron job.
    pub removed_from_queue: bool,
    pub bump_seed: u8,
}

#[account]
pub struct CronJobTransactionV0 {
    pub id: u32,
    pub cron_job: Pubkey,
    pub transaction: TransactionSourceV0,
    pub bump_seed: u8,
}

impl Default for CronJobTransactionV0 {
    fn default() -> Self {
        Self {
            id: 0,
            cron_job: Pubkey::default(),
            transaction: TransactionSourceV0::CompiledV0(CompiledTransactionV0::default()),
            bump_seed: 0,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct TransactionLocation {
    pub offset: u32,
    pub length: u32,
    pub next_free: u32,
}

#[account]
#[derive(Default)]
pub struct CronJobNameMappingV0 {
    pub cron_job: Pubkey,
    pub name: String,
    pub bump_seed: u8,
}
