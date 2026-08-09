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
    #[msg("Cron job already has a scheduled task")]
    TaskAlreadyQueued,
    #[msg("Not enough accounts")]
    NotEnoughAccounts,
    #[msg("Task queue is full")]
    TaskQueueFull,
    #[msg("Not running as a scheduled task")]
    NotRunningAsScheduledTask,
    #[msg("Not the schedule task this cron job records")]
    WrongScheduleTask,
    #[msg("Not a transaction of this cron job")]
    WrongCronTransaction,
}
