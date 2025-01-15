use std::collections::HashMap;

use anchor_lang::{prelude::*, solana_program::instruction::Instruction};

pub mod cron;
declare_id!("tuktukUrfhXT6ZT77QTU8RQtvgL967uRuVagWF57zVA");

declare_program!(tuktuk);

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct RunTaskReturnV0 {
    pub tasks: Vec<TaskReturnV0>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct TaskReturnV0 {
    pub trigger: TriggerV0,
    // Note that you can pass accounts from the remaining accounts to reduce
    // the size of the transaction
    pub transaction: TransactionSourceV0,
    pub crank_reward: Option<u64>,
    // Number of free tasks to append to the end of the accounts. This allows
    // you to easily add new tasks
    pub free_tasks: u8,
}

impl Default for TaskReturnV0 {
    fn default() -> Self {
        TaskReturnV0 {
            trigger: TriggerV0::Now,
            transaction: TransactionSourceV0::CompiledV0(CompiledTransactionV0::default()),
            crank_reward: None,
            free_tasks: 0,
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for TriggerV0 {
    fn default() -> Self {
        TriggerV0::Now
    }
}

pub use self::{
    tuktuk::{
        accounts::{TaskQueueNameMappingV0, TaskQueueV0, TaskV0, TuktukConfigV0},
        client, types,
    },
    types::{
        CompiledInstructionV0, CompiledTransactionV0, InitializeTuktukConfigArgsV0,
        TransactionSourceV0, TriggerV0,
    },
};

impl TriggerV0 {
    pub fn is_active(&self, now: i64) -> bool {
        match self {
            TriggerV0::Now => true,
            TriggerV0::Timestamp(ts) => now >= *ts,
        }
    }
}

impl TaskQueueV0 {
    pub fn task_exists(&self, task_idx: u16) -> bool {
        self.task_bitmap[task_idx as usize / 8] & (1 << (task_idx % 8)) != 0
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

pub fn compile_transaction(
    instructions: Vec<Instruction>,
    signer_seeds: Vec<Vec<Vec<u8>>>,
) -> Result<(CompiledTransactionV0, Vec<AccountMeta>)> {
    let mut pubkeys_to_metadata: HashMap<Pubkey, AccountMeta> = HashMap::new();

    // Process all instructions to build metadata
    for ix in &instructions {
        pubkeys_to_metadata
            .entry(ix.program_id)
            .or_insert(AccountMeta {
                pubkey: ix.program_id,
                is_signer: false,
                is_writable: false,
            });

        for key in &ix.accounts {
            let entry = pubkeys_to_metadata
                .entry(key.pubkey)
                .or_insert(AccountMeta {
                    is_signer: false,
                    is_writable: false,
                    pubkey: key.pubkey,
                });
            entry.is_writable |= key.is_writable;
            entry.is_signer |= key.is_signer;
        }
    }

    // Sort accounts: writable signers first, then ro signers, then rw non-signers, then ro
    let mut sorted_accounts: Vec<Pubkey> = pubkeys_to_metadata.keys().cloned().collect();
    sorted_accounts.sort_by(|a, b| {
        let a_meta = &pubkeys_to_metadata[a];
        let b_meta = &pubkeys_to_metadata[b];

        // Compare accounts based on priority: writable signers > readonly signers > writable > readonly
        fn get_priority(meta: &AccountMeta) -> u8 {
            match (meta.is_signer, meta.is_writable) {
                (true, true) => 0,   // Writable signer: highest priority
                (true, false) => 1,  // Readonly signer
                (false, true) => 2,  // Writable non-signer
                (false, false) => 3, // Readonly non-signer: lowest priority
            }
        }

        get_priority(a_meta).cmp(&get_priority(b_meta))
    });

    // Count different types of accounts
    let mut num_rw_signers = 0u8;
    let mut num_ro_signers = 0u8;
    let mut num_rw = 0u8;

    for k in &sorted_accounts {
        let metadata = &pubkeys_to_metadata[k];
        if metadata.is_signer && metadata.is_writable {
            num_rw_signers += 1;
        } else if metadata.is_signer && !metadata.is_writable {
            num_ro_signers += 1;
        } else if metadata.is_writable {
            num_rw += 1;
        }
    }

    // Create accounts to index mapping
    let accounts_to_index: HashMap<Pubkey, u8> = sorted_accounts
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, i as u8))
        .collect();

    // Compile instructions
    let compiled_instructions: Vec<CompiledInstructionV0> = instructions
        .iter()
        .map(|ix| CompiledInstructionV0 {
            program_id_index: *accounts_to_index.get(&ix.program_id).unwrap(),
            accounts: ix
                .accounts
                .iter()
                .map(|k| *accounts_to_index.get(&k.pubkey).unwrap())
                .collect(),
            data: ix.data.clone(),
        })
        .collect();

    let remaining_accounts = sorted_accounts
        .iter()
        .enumerate()
        .map(|(index, k)| AccountMeta {
            pubkey: *k,
            is_signer: false,
            is_writable: index < num_rw_signers as usize
                || (index >= num_rw_signers as usize + num_ro_signers as usize
                    && index < num_rw_signers as usize + num_ro_signers as usize + num_rw as usize),
        })
        .collect();

    Ok((
        CompiledTransactionV0 {
            num_ro_signers,
            num_rw_signers,
            num_rw,
            instructions: compiled_instructions,
            signer_seeds,
            accounts: sorted_accounts,
        },
        remaining_accounts,
    ))
}
