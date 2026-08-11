use anchor_lang::{prelude::*, Discriminator};
use tuktuk_program::{CompiledTransactionV0, TransactionSourceV0};

use crate::error::ErrorCode;

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
    // Deprecated: You should use the next_schedule_task instead
    // A cron job is removed from the queue when it no longer has enough lamports to fund tasks
    // Once this is set, you need to requeue the cron job.
    pub removed_from_queue: bool,
    pub bump_seed: u8,
    // Pubkey::default() when no task scheduled
    pub next_schedule_task: Pubkey,
}

#[account]
pub struct CronJobTransactionV0 {
    pub id: u32,
    pub cron_job: Pubkey,
    pub transaction: TransactionSourceV0,
    pub bump_seed: u8,
}

impl CronJobTransactionV0 {
    /// Where `id` and `cron_job` sit: the discriminator, then `id: u32`, then `cron_job`.
    /// Pinned by `tests::cron_job_transaction_layout`.
    const ID_OFFSET: usize = 8;
    const CRON_JOB_OFFSET: usize = Self::ID_OFFSET + 4;

    /// Which cron job a record belongs to and which index it holds, read without materialising
    /// the transaction it stores. A schedule run reads one record per task it queues and the
    /// allocator never hands that memory back, so the record is not deserialized whole just to
    /// answer this. `None` when the account holds nothing, which is what a removed record
    /// leaves behind.
    pub fn identity_of(account: &AccountInfo) -> Result<Option<(u32, Pubkey)>> {
        if account.data_is_empty() {
            return Ok(None);
        }
        require_keys_eq!(*account.owner, crate::ID, ErrorCode::WrongCronTransaction);
        let data = account.try_borrow_data()?;
        require_gte!(
            data.len(),
            Self::CRON_JOB_OFFSET + 32,
            ErrorCode::WrongCronTransaction
        );
        require!(
            data.starts_with(Self::DISCRIMINATOR),
            ErrorCode::WrongCronTransaction
        );
        let id = u32::from_le_bytes(
            data[Self::ID_OFFSET..Self::CRON_JOB_OFFSET]
                .try_into()
                .map_err(|_| error!(ErrorCode::WrongCronTransaction))?,
        );
        let cron_job = Pubkey::try_from(&data[Self::CRON_JOB_OFFSET..Self::CRON_JOB_OFFSET + 32])
            .map_err(|_| error!(ErrorCode::WrongCronTransaction))?;

        Ok(Some((id, cron_job)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(owner: &Pubkey, data: &mut [u8]) -> Result<Option<(u32, Pubkey)>> {
        let key = Pubkey::new_unique();
        let mut lamports = 0u64;
        CronJobTransactionV0::identity_of(&AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            data,
            owner,
            false,
            0,
        ))
    }

    fn serialized(cron_job: Pubkey) -> Vec<u8> {
        let mut data = CronJobTransactionV0::DISCRIMINATOR.to_vec();
        CronJobTransactionV0 {
            id: 1,
            cron_job,
            ..Default::default()
        }
        .serialize(&mut data)
        .expect("serialize a record");
        data
    }

    /// Only the contents of a record say which cron job it belongs to, so those contents have
    /// to be ones this program wrote.
    #[test]
    fn identity_of_reads_only_this_program_s_records() {
        let cron_job = Pubkey::new_unique();
        let mut ours = serialized(cron_job);
        assert_eq!(
            record(&crate::ID, &mut ours).expect("a record of ours"),
            Some((1, cron_job)),
        );
        // Same bytes, someone else's account.
        assert!(record(&Pubkey::new_unique(), &mut ours).is_err());
        // Our account, some other kind of record.
        let mut wrong_kind = serialized(cron_job);
        wrong_kind[..8].copy_from_slice(&[0u8; 8]);
        assert!(record(&crate::ID, &mut wrong_kind).is_err());
        // Truncated before the field.
        assert!(record(&crate::ID, &mut ours[..40]).is_err());
        // A removed record leaves the account behind holding nothing.
        assert_eq!(record(&crate::ID, &mut []).expect("an empty record"), None);
    }

    #[test]
    fn cron_job_transaction_layout() {
        let cron_job = Pubkey::new_unique();
        let mut data = CronJobTransactionV0::DISCRIMINATOR.to_vec();
        CronJobTransactionV0 {
            id: 7,
            cron_job,
            bump_seed: 3,
            ..Default::default()
        }
        .serialize(&mut data)
        .expect("serialize a record");
        assert_eq!(
            &data
                [CronJobTransactionV0::CRON_JOB_OFFSET..CronJobTransactionV0::CRON_JOB_OFFSET + 32],
            cron_job.as_ref(),
        );
        assert_eq!(
            &data[CronJobTransactionV0::ID_OFFSET..CronJobTransactionV0::CRON_JOB_OFFSET],
            7u32.to_le_bytes(),
        );
    }
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
