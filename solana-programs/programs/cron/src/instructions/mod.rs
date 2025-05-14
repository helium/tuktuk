use anchor_lang::solana_program::hash::hash;

pub mod add_cron_transaction_v0;
pub mod close_cron_job_v0;
pub mod initialize_cron_job_v0;
pub mod queue_cron_tasks_v0;
pub mod remove_cron_transaction_v0;
pub mod requeue_cron_task_v0;

pub use add_cron_transaction_v0::*;
pub use close_cron_job_v0::*;
pub use initialize_cron_job_v0::*;
pub use queue_cron_tasks_v0::*;
pub use remove_cron_transaction_v0::*;
pub use requeue_cron_task_v0::*;

pub fn hash_name(name: &str) -> [u8; 32] {
    hash(name.as_bytes()).to_bytes()
}
