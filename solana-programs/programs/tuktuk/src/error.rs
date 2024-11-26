use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Task already exists")]
    TaskAlreadyExists,
    #[msg("Signer account mismatched account in definition")]
    InvalidSigner,
    #[msg("Writable account mismatched account in definition")]
    InvalidWritable,
    #[msg("Account mismatched account in definition")]
    InvalidAccount,
    #[msg("Invalid data increase")]
    InvalidDataIncrease,
    #[msg("Task not ready")]
    TaskNotReady,
    #[msg("Task queue not empty")]
    TaskQueueNotEmpty,
}
