use itertools::Itertools;
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::NullSigner,
    transaction::VersionedTransaction,
};

use crate::error::Error;

pub const MAX_TRANSACTION_SIZE: usize = 1232; // Maximum transaction size in bytes

#[derive(Debug)]
pub struct PackedTransaction {
    pub instructions: Vec<Instruction>,
    pub task_ids: Vec<usize>,
}

impl Default for PackedTransaction {
    fn default() -> Self {
        Self {
            instructions: vec![
                // High compute unit limit for simulation, should be updated after simulation
                ComputeBudgetInstruction::set_compute_unit_limit(1000000),
                ComputeBudgetInstruction::set_compute_unit_price(1),
            ],
            task_ids: Default::default(),
        }
    }
}

impl PackedTransaction {
    pub fn push(&mut self, instructions: &[Instruction], index: usize) {
        self.instructions.extend_from_slice(instructions);
        self.task_ids.push(index);
    }

    pub fn is_empty(&self) -> bool {
        self.task_ids.is_empty()
    }

    pub fn mk_transaction(
        &self,
        extra_ixs: &[Instruction],
        lookup_tables: &[AddressLookupTableAccount],
        signers: &[Pubkey],
    ) -> Result<VersionedTransaction, Error> {
        let ixs = &[&self.instructions, extra_ixs].concat();
        v0::Message::try_compile(&signers[0], ixs, lookup_tables, Hash::default())
            .map_err(Error::from)
            .map(VersionedMessage::V0)
            .and_then(|message| {
                VersionedTransaction::try_new(
                    message,
                    &signers.iter().map(NullSigner::new).collect_vec(),
                )
                .map_err(|e| Error::SignerError(e.to_string()))
            })
    }

    pub fn transaction_len(
        &self,
        extra_ixs: &[Instruction],
        lookup_tables: &[AddressLookupTableAccount],
    ) -> Result<usize, Error> {
        let mut signers = self
            .instructions
            .iter()
            .chain(extra_ixs.iter())
            .flat_map(|ix| {
                ix.accounts
                    .iter()
                    .filter(|a| a.is_signer)
                    .map(|a| a.pubkey)
                    .collect_vec()
            })
            .collect_vec();
        if signers.is_empty() {
            signers.push(Pubkey::default());
        }
        let tx = self.mk_transaction(extra_ixs, lookup_tables, &signers)?;
        bincode::serialize(&tx)
            .map(|data| data.len())
            .map_err(|e| Error::SerializationError(e.to_string()))
    }
}

// Returns packed txs with the indices in instructions that were used in that tx.
pub fn pack_instructions_into_transactions(
    instructions: &[&[Instruction]],
    lookup_tables: Option<Vec<AddressLookupTableAccount>>,
) -> Result<Vec<PackedTransaction>, Error> {
    let mut transactions = Vec::new();
    let mut curr_transaction = PackedTransaction::default();
    let lookup_tables = lookup_tables.unwrap_or_default();

    // Instead of flattening all instructions, process them group by group
    for (group_idx, group) in instructions.iter().enumerate() {
        // Create a test transaction with current instructions + entire new group.
        // If adding the entire group would exceed size limit, start a new transaction
        // (but only if we already have instructions in the current batch)
        if curr_transaction.transaction_len(group, &lookup_tables)? > MAX_TRANSACTION_SIZE
            && !curr_transaction.is_empty()
        {
            transactions.push(curr_transaction);
            curr_transaction = PackedTransaction::default();
        }

        // Add the entire group to current transaction
        curr_transaction.push(group, group_idx);

        if curr_transaction.transaction_len(&[], &lookup_tables)? > MAX_TRANSACTION_SIZE {
            return Err(Error::IxGroupTooLarge);
        }
    }

    // Push final transaction if there are remaining instructions
    if !curr_transaction.is_empty() {
        transactions.push(curr_transaction);
    }

    Ok(transactions)
}
