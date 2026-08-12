use itertools::Itertools;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::NullSigner,
    transaction::VersionedTransaction,
};
use tracing::info;

use crate::error::Error;

pub const MAX_RECENT_PRIORITY_FEE_ACCOUNTS: usize = 128;
pub const MIN_PRIORITY_FEE: u64 = 1;

/// The most compute units a transaction may be granted. Asking for more is clamped to this
/// silently, so a request is capped here instead of relying on that.
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;

/// Margin over what a simulation measured.
pub const COMPUTE_MARGIN: f64 = 1.2;

/// The compute budget to request for work a simulation measured at `units_consumed`.
///
/// `None` is not a measurement of zero: it means the simulation reported no consumption, so the
/// whole budget is asked for rather than a margin over nothing.
pub fn compute_budget_from_simulation(units_consumed: Option<u64>) -> u32 {
    let Some(measured) = units_consumed else {
        return MAX_COMPUTE_UNIT_LIMIT;
    };

    ((measured as f64 * COMPUTE_MARGIN) as u64).min(MAX_COMPUTE_UNIT_LIMIT as u64) as u32
}

pub async fn get_estimate<C: AsRef<RpcClient>>(
    client: &C,
    accounts: &[Pubkey],
) -> Result<u64, Error> {
    get_estimate_with_min(client, accounts, MIN_PRIORITY_FEE).await
}

pub async fn get_estimate_with_min<C: AsRef<RpcClient>>(
    client: &C,
    accounts: &[Pubkey],
    min_priority_fee: u64,
) -> Result<u64, Error> {
    let account_keys: Vec<Pubkey> = accounts
        .iter()
        .take(MAX_RECENT_PRIORITY_FEE_ACCOUNTS)
        .cloned()
        .collect();
    let recent_fees = client
        .as_ref()
        .get_recent_prioritization_fees(&account_keys)
        .await?;
    let mut max_per_slot = Vec::new();
    for (slot, fees) in &recent_fees.into_iter().chunk_by(|x| x.slot) {
        let Some(maximum) = fees.map(|x| x.prioritization_fee).max() else {
            continue;
        };
        max_per_slot.push((slot, maximum));
    }
    // Only take the most recent 20 maximum fees:
    max_per_slot.sort_by(|a, b| a.0.cmp(&b.0).reverse());
    let mut max_per_slot: Vec<_> = max_per_slot.into_iter().take(20).map(|x| x.1).collect();
    max_per_slot.sort();
    // Get the median:
    let num_recent_fees = max_per_slot.len();
    let mid = num_recent_fees / 2;
    let estimate = if num_recent_fees == 0 {
        min_priority_fee
    } else if num_recent_fees % 2 == 0 {
        // If the number of samples is even, taken the mean of the two median fees
        (max_per_slot[mid - 1] + max_per_slot[mid]) / 2
    } else {
        max_per_slot[mid]
    }
    .max(min_priority_fee);
    Ok(estimate)
}

pub trait SetPriorityFees {
    fn compute_budget(self, limit: u32) -> Self;
    fn compute_price(self, priority_fee: u64) -> Self;
}

pub fn compute_budget_instruction(compute_limit: u32) -> solana_sdk::instruction::Instruction {
    solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(compute_limit)
}

pub fn compute_price_instruction(priority_fee: u64) -> solana_sdk::instruction::Instruction {
    solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(priority_fee)
}

pub async fn compute_price_instruction_for_accounts<C: AsRef<RpcClient>>(
    client: &C,
    accounts: &[Pubkey],
) -> Result<(solana_sdk::instruction::Instruction, u64), crate::error::Error> {
    let priority_fee = get_estimate(client, accounts).await?;
    Ok((compute_price_instruction(priority_fee), priority_fee))
}

pub async fn compute_budget_for_instructions<C: AsRef<RpcClient>>(
    client: &C,
    instructions: &[Instruction],
    compute_multiplier: f32,
    payer: &Pubkey,
    blockhash: Option<solana_program::hash::Hash>,
    lookup_tables: Option<Vec<AddressLookupTableAccount>>,
) -> Result<(solana_sdk::instruction::Instruction, u32), crate::error::Error> {
    // Check for existing compute unit limit instruction and replace it if found
    let mut updated_instructions = instructions.to_vec();
    let mut has_compute_budget = false;
    for ix in &mut updated_instructions {
        if ix.program_id == solana_sdk::compute_budget::id()
            && ix.data.first()
                == solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(0)
                    .data
                    .first()
        {
            ix.data = solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                MAX_COMPUTE_UNIT_LIMIT,
            )
            .data; // Replace limit
            has_compute_budget = true;
            break;
        }
    }

    if !has_compute_budget {
        // Prepend compute budget instruction if none was found
        updated_instructions.insert(
            0,
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                MAX_COMPUTE_UNIT_LIMIT,
            ),
        );
    }

    let blockhash_actual = match blockhash {
        Some(hash) => hash,
        None => client.as_ref().get_latest_blockhash().await?,
    };
    let message = VersionedMessage::V0(v0::Message::try_compile(
        payer,
        &updated_instructions,
        lookup_tables.unwrap_or_default().as_slice(),
        blockhash_actual,
    )?);
    let num_signers = updated_instructions
        .iter()
        .flat_map(|ix| ix.accounts.iter())
        .filter(|a| a.is_signer)
        .map(|a| a.pubkey)
        .chain(std::iter::once(*payer)) // Include payer
        .unique()
        .count();
    let signers = (0..num_signers)
        .map(|_| NullSigner::new(payer))
        .collect::<Vec<_>>();
    let null_signers: Vec<&NullSigner> = signers.iter().collect();
    let snub_tx =
        VersionedTransaction::try_new(message, null_signers.as_slice()).map_err(Error::signer)?;

    // Simulate the transaction to get the actual compute used
    let simulation_result = client.as_ref().simulate_transaction(&snub_tx).await?;
    // A simulation that failed stopped partway, so what it consumed is not a measurement of the
    // work: scaling it produces a budget too small for the transaction it is about to be spent on.
    let final_compute_budget = if let Some(err) = &simulation_result.value.err {
        info!(?err, ?simulation_result.value.logs, "simulation error");
        MAX_COMPUTE_UNIT_LIMIT
    } else {
        let measured = simulation_result
            .value
            .units_consumed
            .unwrap_or(MAX_COMPUTE_UNIT_LIMIT as u64);
        ((measured as f64 * compute_multiplier as f64) as u64).min(MAX_COMPUTE_UNIT_LIMIT as u64)
            as u32
    };
    Ok((
        compute_budget_instruction(final_compute_budget),
        final_compute_budget,
    ))
}

pub async fn auto_compute_price<C: AsRef<RpcClient>>(
    client: &C,
    instructions: &[Instruction],
    payer: &Pubkey,
    compute_limit: u32,
) -> Result<(Vec<Instruction>, u64), Error> {
    let mut updated_instructions = instructions.to_vec();
    // Compute price instruction
    let accounts: Vec<Pubkey> = instructions
        .iter()
        .flat_map(|i| i.accounts.iter().map(|a| a.pubkey))
        .unique()
        .collect();
    let (compute_price_ix, priority_fee) =
        compute_price_instruction_for_accounts(client, &accounts).await?;

    // Replace or insert compute price instruction
    if let Some(pos) = instructions.iter().position(|ix| {
        ix.program_id == solana_sdk::compute_budget::id()
            && ix.data.first() == compute_price_ix.data.first()
    }) {
        updated_instructions[pos] = compute_price_ix; // Replace existing
    } else {
        updated_instructions.insert(1, compute_price_ix); // Insert after compute budget
    }

    // Count unique signers
    let num_unique_signers = instructions
        .iter()
        .flat_map(|i| i.accounts.iter())
        .filter(|a| a.is_signer)
        .map(|a| a.pubkey)
        .chain(std::iter::once(*payer)) // Include payer
        .unique_by(|pubkey| *pubkey)
        .count();

    // Count ed25519 signatures
    let num_ed25519_sigs = instructions
        .iter()
        .filter(|ix| ix.program_id == solana_sdk::ed25519_program::id())
        .map(|ix| ix.data[0] as usize)
        .sum::<usize>();

    let num_secp_sigs = instructions
        .iter()
        .filter(|ix| ix.program_id == solana_sdk::secp256k1_program::id())
        .map(|ix| ix.data[0] as usize)
        .sum::<usize>();
    Ok((
        updated_instructions,
        // compute fee + signature fees + ed25519 signature fees
        (priority_fee * (compute_limit as u64)).div_ceil(1_000_000)  // Ceiling div
            + (num_unique_signers as u64 * 5000)
            + (num_ed25519_sigs as u64 * 5000)
            + (num_secp_sigs as u64 * 5000),
    ))
}

// Returns the instructions and the total fee in lamports
pub async fn auto_compute_limit_and_price<C: AsRef<RpcClient>>(
    client: &C,
    instructions: &[Instruction],
    compute_multiplier: f32,
    payer: &Pubkey,
    blockhash: Option<solana_program::hash::Hash>,
    lookup_tables: Option<Vec<AddressLookupTableAccount>>,
) -> Result<(Vec<Instruction>, u64), Error> {
    let mut updated_instructions = instructions.to_vec();

    // Compute budget instruction
    let (compute_budget_ix, compute_limit) = compute_budget_for_instructions(
        client,
        &updated_instructions,
        compute_multiplier,
        payer,
        blockhash,
        lookup_tables,
    )
    .await?;

    // Replace or insert compute budget instruction
    if let Some(pos) = updated_instructions.iter().position(|ix| {
        ix.program_id == solana_sdk::compute_budget::id()
            && ix.data.first() == compute_budget_ix.data.first()
    }) {
        updated_instructions[pos] = compute_budget_ix; // Replace existing
    } else {
        updated_instructions.insert(0, compute_budget_ix); // Insert at the beginning
    }

    auto_compute_price(client, &updated_instructions, payer, compute_limit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budget_leaves_the_margin_above_what_was_measured() {
        // Measured against live traffic: a delegation claim simulates around 410,000 and the
        // budget granted for it sits around 490,000, which is the margin and nothing else.
        let budget = compute_budget_from_simulation(Some(410_000));
        assert_eq!(budget, 492_000);
        assert!(budget > 410_000);
    }

    #[test]
    fn nothing_measured_asks_for_the_whole_budget() {
        assert_eq!(compute_budget_from_simulation(None), MAX_COMPUTE_UNIT_LIMIT);
    }

    #[test]
    fn a_budget_never_exceeds_what_a_transaction_may_be_granted() {
        for measured in [
            MAX_COMPUTE_UNIT_LIMIT as u64,
            MAX_COMPUTE_UNIT_LIMIT as u64 * 2,
            u64::MAX,
        ] {
            assert_eq!(
                compute_budget_from_simulation(Some(measured)),
                MAX_COMPUTE_UNIT_LIMIT,
                "measured {measured} was not capped"
            );
        }
    }
}
