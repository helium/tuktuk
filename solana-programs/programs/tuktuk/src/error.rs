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
    #[msg("Free task account not empty")]
    FreeTaskAccountNotEmpty,
    #[msg("Invalid task PDA")]
    InvalidTaskPDA,
    #[msg("Task queue insufficient funds")]
    TaskQueueInsufficientFunds,
    #[msg("Sig verification failed")]
    SigVerificationFailed,
    #[msg("Invalid transaction source")]
    InvalidTransactionSource,
    #[msg("Invalid task verification hash")]
    InvalidVerificationAccountsHash,
    #[msg("Invalid rent refund")]
    InvalidRentRefund,
    #[msg("Invalid task id")]
    InvalidTaskId,
    #[msg("Don't use the dummy instruction")]
    DummyInstruction,
    #[msg("Invalid description length")]
    InvalidDescriptionLength,
    #[msg("Task queue has queue authorities")]
    TaskQueueHasQueueAuthorities,
    #[msg("Free tasks must be less than the capacity of the task queue")]
    FreeTasksGreaterThanCapacity,
    #[msg("Task ID is already in use")]
    TaskIdAlreadyInUse,
    #[msg("Duplicate task IDs provided")]
    DuplicateTaskIds,
    #[msg("Number of free task IDs does not match number of free task accounts")]
    MismatchedFreeTaskCounts,
    #[msg("Too many returned tasks, increase free tasks count")]
    TooManyReturnedTasks,
    #[msg("Malformed remote transaction")]
    MalformedRemoteTransaction,
    #[msg("Invalid account key")]
    InvalidAccountKey,
    #[msg("Invalid crank reward")]
    InvalidCrankReward,
    // Reserved so the follow-up hardening PR can add these without renumbering the codes
    // above, which clients match on.
    #[msg("Signer seeds do not resolve to a valid PDA")]
    InvalidSignerSeeds,
    #[msg("Instruction references an account index that was not provided")]
    InvalidAccountIndex,
    #[msg("Returned tasks account is not owned by the program that returned it")]
    InvalidTasksAccountOwner,
    #[msg("Returned task crank reward exceeds the task queue's min crank reward")]
    CrankRewardExceedsMax,
}
