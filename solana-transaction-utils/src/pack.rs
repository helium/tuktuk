use crate::error::Error;
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

const MAX_TRANSACTION_SIZE: usize = 1232; // Maximum transaction size in bytes

// Returns packed txs with the indices in instructions that were used in that tx.
#[allow(clippy::type_complexity)]
#[allow(clippy::result_large_err)]
pub fn pack_instructions_into_transactions(
    instructions: Vec<Vec<Instruction>>,
    payer: &Keypair,
    lookup_tables: Option<Vec<AddressLookupTableAccount>>,
) -> Result<Vec<(Vec<Instruction>, Vec<usize>)>, Error> {
    // make a transaction from a slice of instructions
    fn mk_transaction(
        ixs: &[Instruction],
        lookup_tables: &[AddressLookupTableAccount],
        payer: &Keypair,
    ) -> Result<VersionedTransaction, Error> {
        v0::Message::try_compile(&payer.pubkey(), ixs, lookup_tables, Hash::default())
            .map_err(Error::from)
            .map(VersionedMessage::V0)
            .and_then(|message| {
                VersionedTransaction::try_new(message, &[&NullSigner::new(&payer.pubkey())])
                    .map_err(Error::from)
            })
    }
    // get the lenth in bytes for a given txn
    fn transaction_len(tx: &VersionedTransaction) -> Result<usize, Error> {
        bincode::serialize(tx)
            .map(|data| data.len())
            .map_err(Error::from)
    }
    let mut transactions = Vec::new();
    let compute_ixs = &[
        ComputeBudgetInstruction::set_compute_unit_limit(200000),
        ComputeBudgetInstruction::set_compute_unit_price(1),
    ];
    let mut curr_instructions: Vec<Instruction> = compute_ixs.to_vec();
    let mut curr_indices: Vec<usize> = Vec::new();
    let lookup_tables = lookup_tables.unwrap_or_default();

    // Instead of flattening all instructions, process them group by group
    for (group_idx, group) in instructions.into_iter().enumerate() {
        // Create a test transaction with current instructions + entire new group
        let test_tx = mk_transaction(
            &[&curr_instructions, group.as_slice()].concat(),
            &lookup_tables,
            payer,
        )?;

        // If adding the entire group would exceed size limit, start a new transaction
        // (but only if we already have instructions in the current batch)
        if transaction_len(&test_tx)? > MAX_TRANSACTION_SIZE && !curr_indices.is_empty() {
            transactions.push((curr_instructions, curr_indices.clone()));
            curr_instructions = compute_ixs.to_vec();
            curr_indices.clear();
        }

        // Add the entire group to current transaction
        curr_indices.push(group_idx);
        curr_instructions.extend(group);

        let tx = mk_transaction(&curr_instructions, &lookup_tables, payer)?;
        if transaction_len(&tx)? > MAX_TRANSACTION_SIZE {
            return Err(Error::IxGroupTooLarge);
        }
    }

    // Push final transaction if there are remaining instructions
    if !curr_instructions.len() > compute_ixs.len() {
        transactions.push((curr_instructions, curr_indices));
    }

    Ok(transactions)
}
