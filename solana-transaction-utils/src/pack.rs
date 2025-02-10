use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    signature::{Keypair, NullSigner},
    signer::Signer,
    transaction::VersionedTransaction,
};

use crate::error::Error;

const MAX_TRANSACTION_SIZE: usize = 1232; // Maximum transaction size in bytes

// Returns packed txs with the indices in instructions that were used in that tx.
pub fn pack_instructions_into_transactions(
    instructions: Vec<Vec<Instruction>>,
    payer: &Keypair,
    lookup_tables: Option<Vec<AddressLookupTableAccount>>,
) -> Result<Vec<(Vec<Instruction>, Vec<usize>)>, Error> {
    let mut transactions = Vec::new();
    let compute_ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(200000),
        ComputeBudgetInstruction::set_compute_unit_price(1),
    ];
    let mut curr_instructions: Vec<Instruction> = compute_ixs.clone();
    let mut curr_indices: Vec<usize> = Vec::new();

    // Instead of flattening all instructions, process them group by group
    for (group_idx, group) in instructions.iter().enumerate() {
        // Create a test transaction with current instructions + entire new group
        let mut test_instructions = curr_instructions.clone();
        test_instructions.extend(group.iter().cloned());
        let message = VersionedMessage::V0(v0::Message::try_compile(
            &payer.pubkey(),
            &test_instructions,
            &lookup_tables.clone().unwrap_or_default(),
            Hash::default(),
        )?);
        let test_tx = VersionedTransaction::try_new(message, &[&NullSigner::new(&payer.pubkey())])?;
        let test_len = bincode::serialize(&test_tx).unwrap().len();

        // If adding the entire group would exceed size limit, start a new transaction
        // (but only if we already have instructions in the current batch)
        if test_len > MAX_TRANSACTION_SIZE && !curr_indices.is_empty() {
            transactions.push((curr_instructions.clone(), curr_indices.clone()));
            curr_instructions = compute_ixs.clone();
            curr_indices.clear();
        }

        // Add the entire group to current transaction
        curr_instructions.extend(group.iter().cloned());
        curr_indices.extend(vec![group_idx; group.len()]);

        // If this single group alone exceeds transaction size, we have a problem
        let message = VersionedMessage::V0(v0::Message::try_compile(
            &payer.pubkey(),
            &curr_instructions,
            &lookup_tables.clone().unwrap_or_default(),
            Hash::default(),
        )?);
        let tx = VersionedTransaction::try_new(message, &[&NullSigner::new(&payer.pubkey())])?;
        let len = bincode::serialize(&tx).unwrap().len();
        if len > MAX_TRANSACTION_SIZE {
            return Err(Error::IxGroupTooLarge);
        }
    }

    // Push final transaction if there are remaining instructions
    if !curr_instructions.is_empty() {
        transactions.push((curr_instructions.clone(), curr_indices.clone()));
    }

    Ok(transactions)
}
