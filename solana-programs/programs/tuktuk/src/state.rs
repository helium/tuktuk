use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};

#[account]
#[derive(Default, InitSpace)]
pub struct TuktukConfigV0 {
    pub min_task_queue_id: u32,
    pub next_task_queue_id: u32,
    pub authority: Pubkey,
    // Minimum sol deposit to create a task queue.
    // We want to minimize the number of task queues, as they are expensive to watch
    // and we want to encourage people to use the same task queue for multiple tasks.
    pub min_deposit: u64,
    pub bump_seed: u8,
}

#[account]
#[derive(Default)]
pub struct TaskQueueAuthorityV0 {
    pub task_queue: Pubkey,
    pub queue_authority: Pubkey,
    pub bump_seed: u8,
}

#[account]
#[derive(Default)]
pub struct TaskQueueV0 {
    pub tuktuk_config: Pubkey,
    pub id: u32,
    pub update_authority: Pubkey,
    pub reserved: Pubkey,
    pub min_crank_reward: u64,
    pub uncollected_protocol_fees: u64,
    pub capacity: u16,
    pub created_at: i64,
    pub updated_at: i64,
    pub bump_seed: u8,
    // A 1 in this bitmap indicates there's a job at that ID, a 0 indicates there's not. Each idx corresponds to an ID.
    pub task_bitmap: Vec<u8>,
    pub name: String,
    pub lookup_tables: Vec<Pubkey>,
    pub num_queue_authorities: u16,
    // Age before a task is considered stale and can be run/deleted without running the instructions.
    // The longer this value, the more likely you have stale tasks clogging up your queue, which can cause
    // the queue to be full and prevent new tasks from being added.
    // The shorter this value, the more difficult it will be to debug, as failed tasks dissappear.
    pub stale_task_age: u32,
}

/// Header portion of TaskQueueV0 for memory-efficient access
#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct TaskQueueHeader {
    pub tuktuk_config: Pubkey,
    pub id: u32,
    pub update_authority: Pubkey,
    pub reserved: Pubkey,
    pub min_crank_reward: u64,
    pub uncollected_protocol_fees: u64,
    pub capacity: u16,
    pub created_at: i64,
    pub updated_at: i64,
    pub bump_seed: u8,
    pub bitmap_size: u32,
}

#[macro_export]
macro_rules! task_queue_seeds {
    ($task_queue:expr) => {
        &[
            b"task_queue".as_ref(),
            $task_queue.tuktuk_config.as_ref(),
            $task_queue.id.to_le_bytes().as_ref(),
            &[$task_queue.bump_seed],
        ]
    };
}

#[macro_export]
macro_rules! task_seeds {
    ($task:expr) => {
        &[
            b"task".as_ref(),
            $task.task_queue.as_ref(),
            $task.id.to_le_bytes().as_ref(),
            &[$task.bump_seed],
        ]
    };
}

impl TaskQueueV0 {
    pub fn task_exists(&self, task_idx: u16) -> bool {
        self.task_bitmap[task_idx as usize / 8] & (1 << (task_idx % 8)) != 0
    }

    pub fn set_task_exists(&mut self, task_idx: u16, exists: bool) {
        if exists {
            self.task_bitmap[task_idx as usize / 8] |= 1 << (task_idx % 8);
        } else {
            self.task_bitmap[task_idx as usize / 8] &= !(1 << (task_idx % 8));
        }
    }

    pub fn next_available_task_id(&self) -> Option<u16> {
        for (byte_idx, byte) in self.task_bitmap.iter().enumerate() {
            if *byte != 0xff {
                // If byte is not all 1s
                for bit_idx in 0..8 {
                    if byte & (1 << bit_idx) == 0 {
                        return Some((byte_idx * 8 + bit_idx) as u16);
                    }
                }
            }
        }
        None
    }
}

/// Memory-efficient wrapper for TaskQueueV0 that avoids deserializing the entire bitmap
pub struct TaskQueueDataWrapper<'a> {
    data: &'a mut [u8],
    header: TaskQueueHeader,
    bitmap_offset: usize,
    name_offset: usize,
    lookup_tables_offset: usize,
    stale_task_age_offset: usize,
}

impl<'a> TaskQueueDataWrapper<'a> {
    /// The size of the Anchor discriminator (8 bytes)
    pub const DISCRIMINATOR_SIZE: usize = 8;
    /// The size of the TaskQueueHeader in bytes (32+4+32+32+8+8+2+8+8+1+4 = 139 bytes)
    pub const HEADER_SIZE: usize = 139;
    /// Total offset to the bitmap
    pub const BITMAP_OFFSET: usize = Self::DISCRIMINATOR_SIZE + Self::HEADER_SIZE;

    /// Create a new wrapper from account data
    /// Validates the discriminator and deserializes only the header
    pub fn new(data: &'a mut [u8]) -> Result<Self> {
        // Verify we have enough data for discriminator + header
        require_gte!(
            data.len(),
            Self::BITMAP_OFFSET,
            anchor_lang::error::ErrorCode::AccountDidNotDeserialize
        );

        // Verify the discriminator matches TaskQueueV0
        require!(
            &data[0..8] == TaskQueueV0::DISCRIMINATOR,
            anchor_lang::error::ErrorCode::AccountDiscriminatorMismatch
        );

        // Deserialize only the header
        let header =
            TaskQueueHeader::deserialize(&mut &data[Self::DISCRIMINATOR_SIZE..Self::BITMAP_OFFSET])
                .map_err(|_| anchor_lang::error::ErrorCode::AccountDidNotDeserialize)?;

        // Calculate offsets for fields after the bitmap
        let bitmap_size = header.bitmap_size as usize;
        let name_offset = Self::BITMAP_OFFSET + bitmap_size;

        // Read name length (4 bytes) and calculate name data size
        let name_len = u32::from_le_bytes([
            data[name_offset],
            data[name_offset + 1],
            data[name_offset + 2],
            data[name_offset + 3],
        ]) as usize;
        let lookup_tables_offset = name_offset + 4 + name_len;

        // Read lookup tables length (4 bytes) and calculate lookup tables data size
        let lookup_tables_len = u32::from_le_bytes([
            data[lookup_tables_offset],
            data[lookup_tables_offset + 1],
            data[lookup_tables_offset + 2],
            data[lookup_tables_offset + 3],
        ]) as usize;
        let stale_task_age_offset = lookup_tables_offset + 4 + (lookup_tables_len * 32) + 2; // +2 for num_queue_authorities

        Ok(Self {
            data,
            header,
            bitmap_offset: Self::BITMAP_OFFSET,
            name_offset,
            lookup_tables_offset,
            stale_task_age_offset,
        })
    }

    /// Get reference to the header
    pub fn header(&self) -> &TaskQueueHeader {
        &self.header
    }

    /// Get mutable reference to the header
    pub fn header_mut(&mut self) -> &mut TaskQueueHeader {
        &mut self.header
    }

    /// Check if a task exists at the given index
    pub fn task_exists(&self, task_idx: u16) -> bool {
        let byte_idx = self.bitmap_offset + (task_idx as usize / 8);
        let bit_idx = task_idx % 8;
        self.data[byte_idx] & (1 << bit_idx) != 0
    }

    /// Set whether a task exists at the given index
    pub fn set_task_exists(&mut self, task_idx: u16, exists: bool) {
        let byte_idx = self.bitmap_offset + (task_idx as usize / 8);
        let bit_idx = task_idx % 8;
        if exists {
            self.data[byte_idx] |= 1 << bit_idx;
        } else {
            self.data[byte_idx] &= !(1 << bit_idx);
        }
    }

    /// Get the stale task age
    pub fn stale_task_age(&self) -> u32 {
        let bytes = &self.data[self.stale_task_age_offset..self.stale_task_age_offset + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Set the stale task age
    pub fn set_stale_task_age(&mut self, age: u32) {
        let bytes = age.to_le_bytes();
        self.data[self.stale_task_age_offset..self.stale_task_age_offset + 4]
            .copy_from_slice(&bytes);
    }

    /// Persist the header changes back to the data
    pub fn save(&mut self) -> Result<()> {
        let mut header_bytes = Vec::new();
        self.header
            .serialize(&mut header_bytes)
            .map_err(|_| anchor_lang::error::ErrorCode::AccountDidNotSerialize)?;

        self.data[Self::DISCRIMINATOR_SIZE..Self::BITMAP_OFFSET].copy_from_slice(&header_bytes);

        Ok(())
    }
}

#[account]
#[derive(Default, InitSpace)]
pub struct TaskQueueNameMappingV0 {
    pub task_queue: Pubkey,
    #[max_len(32)]
    pub name: String,
    pub bump_seed: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub enum TransactionSourceV0 {
    CompiledV0(CompiledTransactionV0),
    RemoteV0 { url: String, signer: Pubkey },
}

impl Default for TransactionSourceV0 {
    fn default() -> Self {
        TransactionSourceV0::CompiledV0(CompiledTransactionV0::default())
    }
}

#[account]
#[derive(Default)]
pub struct TaskV0 {
    pub task_queue: Pubkey,
    pub rent_amount: u64,
    pub crank_reward: u64,
    pub id: u16,
    pub trigger: TriggerV0,
    pub rent_refund: Pubkey,
    pub transaction: TransactionSourceV0,
    pub queued_at: i64,
    pub bump_seed: u8,
    pub free_tasks: u8,
    pub description: String,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub enum TriggerV0 {
    #[default]
    Now,
    Timestamp(i64),
}

impl TriggerV0 {
    pub fn is_active(&self) -> Result<bool> {
        match *self {
            TriggerV0::Now => Ok(true),
            TriggerV0::Timestamp(ts) => {
                let current_ts = Clock::get()?.unix_timestamp;
                Ok(ts <= current_ts)
            }
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct CompiledInstructionV0 {
    /// Index into the transaction keys array indicating the program account that executes this instruction.
    pub program_id_index: u8,
    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program.
    pub accounts: Vec<u8>,
    /// The program input data.
    pub data: Vec<u8>,
}

impl CompiledInstructionV0 {
    pub fn size(&self) -> usize {
        1 + 4 + self.accounts.len() + 4 + self.data.len()
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct CompiledTransactionV0 {
    // Accounts are ordered as follows:
    // 1. Writable signer accounts
    // 2. Read only signer accounts
    // 3. writable accounts
    // 4. read only accounts
    pub num_rw_signers: u8,
    pub num_ro_signers: u8,
    pub num_rw: u8,
    pub accounts: Vec<Pubkey>,
    pub instructions: Vec<CompiledInstructionV0>,
    /// Additional signer seeds. Should include bump. Useful for things like initializing a mint where
    /// you cannot pass a keypair.
    /// Note that these seeds will be prefixed with "custom", task_queue.key
    /// and the bump you pass and account should be consistent with this. But to save space
    /// in the instruction, they should be ommitted here. See tests for examples
    pub signer_seeds: Vec<Vec<Vec<u8>>>,
}

impl CompiledTransactionV0 {
    pub fn size(&self) -> usize {
        // Calculate the size of the transaction header (3 u8 fields + 1 byte for instruction count)
        let header_size = 3 + 1;

        // Calculate the maximum account index across all instructions
        let max_accounts = 1 + self
            .instructions
            .iter()
            .flat_map(|i| i.accounts.iter())
            .max()
            .copied()
            .unwrap_or(self.accounts.len() as u8) as usize;

        // Calculate the size of all accounts (4 bytes for length + each Pubkey is 32 bytes)
        let accounts_size = 4 + max_accounts * 32;

        // Calculate the size of all instructions (4 bytes for length + instruction data)
        let instructions_size = 4 + self.instructions.iter().map(|i| i.size()).sum::<usize>();

        header_size + accounts_size + instructions_size
    }
}

impl TransactionSourceV0 {
    pub fn size(&self) -> usize {
        match self {
            TransactionSourceV0::CompiledV0(compiled_tx) => 4 + compiled_tx.size(),
            TransactionSourceV0::RemoteV0 { url, signer: _ } => {
                // For remote transactions, we need to account for the URL string length
                // and the signer pubkey (32 bytes)
                4 + 32 + 4 + url.len() // 4 bytes for string length prefix
            }
        }
    }
}
