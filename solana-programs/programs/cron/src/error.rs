use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid schedule")]
    InvalidSchedule,
    #[msg("Transaction already exists")]
    TransactionAlreadyExists,
    #[msg("Insufficient funds")]
    InsufficientFunds,
    #[msg("Overflow")]
    Overflow,
    #[msg("Invalid data increase")]
    InvalidDataIncrease,
    #[msg("Cron job has transactions")]
    CronJobHasTransactions,
    #[msg("Invalid number of tasks per queue call")]
    InvalidNumTasksPerQueueCall,
    #[msg("Too early to queue tasks")]
    TooEarly,
    #[msg("Next schedule task does not match the one recorded on the cron job")]
    InvalidNextScheduleTask,
    #[msg("Cron job already has a schedule task queued")]
    TaskAlreadyScheduled,
    #[msg("Schedule has no next execution time")]
    NoNextExecutionTime,
    #[msg("Not enough accounts were provided for the tasks to queue")]
    NotEnoughAccounts,
}
