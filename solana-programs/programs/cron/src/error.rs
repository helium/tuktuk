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
}
