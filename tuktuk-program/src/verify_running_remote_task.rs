use anchor_lang::{
    prelude::*,
    solana_program::sysvar::instructions::{load_current_index_checked, load_instruction_at_checked},
    Discriminator,
};

use crate::{tuktuk, TaskV0, TransactionSourceV0};

/// `tuktuk::run_task_v0`, whose account list names the task it is running. Both are pinned by
/// `tests::run_task_v0_shape` against the client this crate is built with.
const RUN_TASK_V0_TASK_ACCOUNT: usize = 3;

#[error_code]
pub enum VerifyRunningRemoteTaskError {
    #[msg("The current top-level instruction is not tuktuk run_task_v0")]
    NotRunningAsTask,
    #[msg("The task account is not the one run_task_v0 is running")]
    TaskMismatch,
    #[msg("The task account is not owned by tuktuk")]
    InvalidTaskOwner,
    #[msg("The task is not a RemoteV0 task signed by the expected signer")]
    NotRemoteTaskFromSigner,
}

/// Proves the current top-level instruction is tuktuk `run_task_v0` executing `task`, and that
/// `task` is a `RemoteV0` task whose transaction was signed by `expected_signer`.
///
/// tuktuk only checks the ed25519 instruction ahead of `run_task_v0` when the task it is running
/// is `RemoteV0`, and its `verification_hash` binds that signature to the task account and the
/// accounts passed. A program CPI'd by a task therefore cannot trust an ed25519 instruction on
/// its own: it has to know the task being run is a remote one, which is what this proves. Once it
/// holds, every instruction `run_task_v0` issues came out of a message `expected_signer` signed
/// for this exact task.
pub fn verify_running_remote_task(
    sysvar_instructions: &AccountInfo,
    task: &AccountInfo,
    expected_signer: &Pubkey,
) -> Result<TaskV0> {
    let index = load_current_index_checked(sysvar_instructions)?;
    let ix = load_instruction_at_checked(index as usize, sysvar_instructions)?;
    require_keys_eq!(
        ix.program_id,
        tuktuk::ID,
        VerifyRunningRemoteTaskError::NotRunningAsTask
    );
    require!(
        ix.data.starts_with(tuktuk::client::args::RunTaskV0::DISCRIMINATOR),
        VerifyRunningRemoteTaskError::NotRunningAsTask
    );
    let running_task = ix
        .accounts
        .get(RUN_TASK_V0_TASK_ACCOUNT)
        .ok_or_else(|| error!(VerifyRunningRemoteTaskError::NotRunningAsTask))?
        .pubkey;
    require_keys_eq!(
        running_task,
        task.key(),
        VerifyRunningRemoteTaskError::TaskMismatch
    );
    require_keys_eq!(
        *task.owner,
        tuktuk::ID,
        VerifyRunningRemoteTaskError::InvalidTaskOwner
    );
    let task = TaskV0::try_deserialize(&mut &task.try_borrow_data()?[..])?;
    match task.transaction {
        TransactionSourceV0::RemoteV0 { ref signer, .. } if signer == expected_signer => Ok(task),
        _ => Err(error!(VerifyRunningRemoteTaskError::NotRemoteTaskFromSigner)),
    }
}

#[cfg(test)]
mod tests {
    use anchor_lang::ToAccountMetas;

    use super::*;

    #[test]
    fn run_task_v0_shape() {
        let task = Pubkey::new_unique();
        let metas = tuktuk::client::accounts::RunTaskV0 {
            crank_turner: Pubkey::new_unique(),
            rent_refund: Pubkey::new_unique(),
            task_queue: Pubkey::new_unique(),
            task,
            system_program: Pubkey::new_unique(),
            sysvar_instructions: Pubkey::new_unique(),
        }
        .to_account_metas(None);
        assert_eq!(metas[RUN_TASK_V0_TASK_ACCOUNT].pubkey, task);
    }
}
