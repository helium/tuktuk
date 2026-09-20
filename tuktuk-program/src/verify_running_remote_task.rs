use anchor_lang::{
    prelude::*,
    solana_program::sysvar::instructions::{
        load_current_index_checked, load_instruction_at_checked,
    },
    Discriminator,
};

use crate::{tuktuk, TaskV0, TransactionSourceV0};

/// `tuktuk::run_task_v0`, whose account list names the task it is running. Both are pinned by
/// `tests::run_task_v0_shape` against the client this crate is built with.
const RUN_TASK_V0_TASK_ACCOUNT: usize = 3;

/// Offset above anchor's default 6000 so these codes do not collide with the first variants of
/// the consuming program's own `#[error_code]`, which IDL-based clients decode by number.
#[error_code(offset = 9200)]
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
/// `task` currently deserializes as a `RemoteV0` task whose transaction was signed by
/// `expected_signer`.
///
/// For a `RemoteV0` task, `run_task_v0` verifies the ed25519 instruction ahead of it and checks
/// that its `verification_hash` binds the signature to the task account and the account list. This
/// helper re-reads the task account to confirm it is that kind of task; `dequeue_task_v0` refuses
/// to close the task `run_task_v0` is running, so the shape reported here cannot be forged mid-run
/// by a `CompiledV0` task that re-creates its own account as a `RemoteV0` one.
///
/// This proves the *shape* of the running task, not the *content* of any one instruction. The
/// `verification_hash` binds the task account and the account list, but not the instruction data,
/// so a consumer that writes a value derived from task content (an amount, a chosen recipient) must
/// additionally bind that value to the signed message -- re-deriving the `verification_hash` over
/// the running task and its accounts, and requiring the signed transaction to carry the write it is
/// making -- rather than trusting the shape alone.
///
/// `run_task_v0` must be the top-level instruction. A flow that reaches it through a CPI fails
/// with `NotRunningAsTask`, since the instructions sysvar only exposes top-level instructions.
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
        ix.data
            .starts_with(tuktuk::client::args::RunTaskV0::DISCRIMINATOR),
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
        _ => Err(error!(
            VerifyRunningRemoteTaskError::NotRemoteTaskFromSigner
        )),
    }
}

#[cfg(test)]
mod tests {
    use anchor_lang::{
        solana_program::{
            instruction::{AccountMeta, Instruction},
            sysvar::instructions::{
                construct_instructions_data, BorrowedAccountMeta, BorrowedInstruction, ID as IX_ID,
            },
        },
        ToAccountMetas,
    };

    use super::*;
    use crate::{CompiledTransactionV0, TriggerV0};

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

    /// The instructions sysvar as the runtime would present it for one top-level instruction.
    fn sysvar_holding(ix: &Instruction) -> Vec<u8> {
        let borrowed = BorrowedInstruction {
            program_id: &ix.program_id,
            accounts: ix
                .accounts
                .iter()
                .map(|m| BorrowedAccountMeta {
                    pubkey: &m.pubkey,
                    is_signer: m.is_signer,
                    is_writable: m.is_writable,
                })
                .collect(),
            data: &ix.data,
        };
        let mut data = construct_instructions_data(&[borrowed]);
        let end = data.len() - 2;
        data[end..].copy_from_slice(&0u16.to_le_bytes());
        data
    }

    fn run_task_ix(program_id: Pubkey, discriminator: &[u8], task: Pubkey) -> Instruction {
        Instruction {
            program_id,
            accounts: (0..6)
                .map(|i| {
                    AccountMeta::new_readonly(
                        if i == RUN_TASK_V0_TASK_ACCOUNT {
                            task
                        } else {
                            Pubkey::new_unique()
                        },
                        false,
                    )
                })
                .collect(),
            data: discriminator.to_vec(),
        }
    }

    fn tuktuk_run_task_ix(task: Pubkey) -> Instruction {
        run_task_ix(
            tuktuk::ID,
            tuktuk::client::args::RunTaskV0::DISCRIMINATOR,
            task,
        )
    }

    fn task_with(transaction: TransactionSourceV0) -> Vec<u8> {
        let task = TaskV0 {
            task_queue: Pubkey::new_unique(),
            rent_amount: 0,
            crank_reward: 0,
            id: 0,
            trigger: TriggerV0::Now,
            rent_refund: Pubkey::new_unique(),
            transaction,
            queued_at: 0,
            bump_seed: 0,
            free_tasks: 0,
            description: String::new(),
        };
        let mut data = Vec::new();
        task.try_serialize(&mut data).unwrap();
        data
    }

    fn remote_task(signer: Pubkey) -> Vec<u8> {
        task_with(TransactionSourceV0::RemoteV0 {
            url: "https://example.com".to_string(),
            signer,
        })
    }

    struct Case {
        ix: Instruction,
        task_key: Pubkey,
        task_owner: Pubkey,
        task_data: Vec<u8>,
        expected_signer: Pubkey,
    }

    impl Case {
        /// A run tuktuk would accept: `run_task_v0` on top, running a tuktuk-owned `RemoteV0`
        /// task whose signer is the one we expect.
        fn valid() -> Self {
            let task_key = Pubkey::new_unique();
            let signer = Pubkey::new_unique();
            Case {
                ix: tuktuk_run_task_ix(task_key),
                task_key,
                task_owner: tuktuk::ID,
                task_data: remote_task(signer),
                expected_signer: signer,
            }
        }

        fn verify(self) -> Result<TaskV0> {
            let mut sysvar_data = sysvar_holding(&self.ix);
            let sysvar_owner = Pubkey::default();
            let mut sysvar_lamports = 0u64;
            let sysvar = AccountInfo::new(
                &IX_ID,
                false,
                false,
                &mut sysvar_lamports,
                &mut sysvar_data,
                &sysvar_owner,
                false,
                0,
            );
            let mut task_data = self.task_data;
            let mut task_lamports = 0u64;
            let task = AccountInfo::new(
                &self.task_key,
                false,
                false,
                &mut task_lamports,
                &mut task_data,
                &self.task_owner,
                false,
                0,
            );
            verify_running_remote_task(&sysvar, &task, &self.expected_signer)
        }
    }

    fn assert_fails_with(case: Case, expected: VerifyRunningRemoteTaskError) {
        assert_eq!(case.verify().unwrap_err(), Error::from(expected));
    }

    #[test]
    fn accepts_remote_task_run_by_tuktuk() {
        let case = Case::valid();
        let signer = case.expected_signer;
        let task = case.verify().unwrap();
        match task.transaction {
            TransactionSourceV0::RemoteV0 { signer: s, .. } => assert_eq!(s, signer),
            _ => panic!("expected RemoteV0"),
        }
    }

    #[test]
    fn rejects_top_level_from_another_program() {
        let mut case = Case::valid();
        case.ix.program_id = Pubkey::new_unique();
        assert_fails_with(case, VerifyRunningRemoteTaskError::NotRunningAsTask);
    }

    #[test]
    fn rejects_other_tuktuk_instruction() {
        let mut case = Case::valid();
        case.ix.data = tuktuk::client::args::QueueTaskV0::DISCRIMINATOR.to_vec();
        assert_fails_with(case, VerifyRunningRemoteTaskError::NotRunningAsTask);
    }

    #[test]
    fn rejects_run_task_with_too_few_accounts() {
        let mut case = Case::valid();
        case.ix.accounts.truncate(RUN_TASK_V0_TASK_ACCOUNT);
        assert_fails_with(case, VerifyRunningRemoteTaskError::NotRunningAsTask);
    }

    #[test]
    fn rejects_task_that_is_not_the_one_being_run() {
        let mut case = Case::valid();
        case.ix = tuktuk_run_task_ix(Pubkey::new_unique());
        assert_fails_with(case, VerifyRunningRemoteTaskError::TaskMismatch);
    }

    #[test]
    fn rejects_task_not_owned_by_tuktuk() {
        let mut case = Case::valid();
        case.task_owner = Pubkey::new_unique();
        assert_fails_with(case, VerifyRunningRemoteTaskError::InvalidTaskOwner);
    }

    #[test]
    fn rejects_tuktuk_account_that_is_not_a_task() {
        let mut case = Case::valid();
        // A tuktuk-owned account with some other discriminator, e.g. a task queue.
        case.task_data[..8].copy_from_slice(&[0u8; 8]);
        assert_eq!(
            case.verify().unwrap_err(),
            Error::from(ErrorCode::AccountDiscriminatorMismatch)
        );
    }

    #[test]
    fn rejects_compiled_task() {
        let mut case = Case::valid();
        case.task_data = task_with(TransactionSourceV0::CompiledV0(
            CompiledTransactionV0::default(),
        ));
        assert_fails_with(case, VerifyRunningRemoteTaskError::NotRemoteTaskFromSigner);
    }

    #[test]
    fn rejects_remote_task_from_another_signer() {
        let mut case = Case::valid();
        case.task_data = remote_task(Pubkey::new_unique());
        assert_fails_with(case, VerifyRunningRemoteTaskError::NotRemoteTaskFromSigner);
    }

    #[test]
    fn error_codes_sit_above_consumer_range() {
        assert_eq!(
            u32::from(VerifyRunningRemoteTaskError::NotRunningAsTask),
            9200
        );
    }
}
