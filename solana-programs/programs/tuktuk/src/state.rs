use anchor_lang::prelude::*;

#[account]
#[derive(Default, InitSpace)]
pub struct TuktukConfigV0 {
    pub network_mint: Pubkey,
    pub min_task_queue_id: u32,
    pub next_task_queue_id: u32,
    pub authority: Pubkey,
    pub min_deposit: u64,
    pub bump_seed: u8,
}

#[account]
#[derive(Default)]
pub struct TaskQueueV0 {
    pub tuktuk_config: Pubkey,
    pub id: u32,
    pub update_authority: Pubkey,
    pub queue_authority: Pubkey,
    pub rewards_source: Pubkey,
    pub default_crank_reward: u64,
    pub capacity: u16,
    pub created_at: i64,
    pub updated_at: i64,
    pub bump_seed: u8,
    // A 1 in this bitmap indicates there's a job at that ID, a 0 indicates there's not. Each idx corresponds to an ID.
    pub task_bitmap: Vec<u8>,
    pub name: String,
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

impl TaskQueueV0 {
    pub fn task_exists(&self, task_idx: usize) -> bool {
        self.task_bitmap[task_idx / 8] & (1 << (task_idx % 8)) != 0
    }

    pub fn set_task_exists(&mut self, task_idx: usize, exists: bool) {
        if exists {
            self.task_bitmap[task_idx / 8] |= 1 << (task_idx % 8);
        } else {
            self.task_bitmap[task_idx / 8] &= !(1 << (task_idx % 8));
        }
    }

    pub fn next_available_task_id(&self) -> Option<usize> {
        for (byte_idx, byte) in self.task_bitmap.iter().enumerate() {
            if *byte != 0xff {
                // If byte is not all 1s
                for bit_idx in 0..8 {
                    if byte & (1 << bit_idx) == 0 {
                        return Some(byte_idx * 8 + bit_idx);
                    }
                }
            }
        }
        None
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

#[account]
#[derive(Default)]
pub struct TaskV0 {
    pub task_queue: Pubkey,
    pub crank_reward: u64,
    pub id: u16,
    pub trigger: TriggerV0,
    pub rent_refund: Pubkey,
    pub transaction: CompiledTransactionV0,
    pub queued_at: i64,
    pub bump_seed: u8,
    // Number of free tasks to append to the end of the accounts. This allows
    // you to easily add new tasks
    pub free_tasks: u8,
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
        1 + self.accounts.len() + self.data.len()
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
        1 + self.accounts.len() + self.instructions.iter().map(|i| i.size()).sum::<usize>()
    }
}
