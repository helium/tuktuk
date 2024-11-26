use itertools::Itertools;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, message::Message, pubkey::Pubkey, signers::Signers,
    transaction::Transaction,
};

use crate::error::Error;

pub const MAX_RECENT_PRIORITY_FEE_ACCOUNTS: usize = 128;
pub const MIN_PRIORITY_FEE: u64 = 1;

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

pub async fn compute_budget_for_instructions<C: AsRef<RpcClient>, T: Signers + ?Sized>(
    client: &C,
    instructions: Vec<Instruction>,
    signers: &T,
    compute_multiplier: f32,
    payer: Option<&Pubkey>,
    blockhash: Option<solana_program::hash::Hash>,
) -> Result<(solana_sdk::instruction::Instruction, u32), crate::error::Error> {
    // Check for existing compute unit limit instruction and replace it if found
    let mut updated_instructions = instructions.clone();
    for ix in &mut updated_instructions {
        if ix.program_id == solana_sdk::compute_budget::id()
            && ix.data.first()
                == solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(0)
                    .data
                    .first()
        {
            ix.data = solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                1900000,
            )
            .data; // Replace limit
        }
    }
    let blockhash_actual = match blockhash {
        Some(hash) => hash,
        None => client.as_ref().get_latest_blockhash().await?,
    };
    let snub_tx = Transaction::new(
        signers,
        Message::new(&updated_instructions, payer),
        blockhash_actual,
    );

    // Simulate the transaction to get the actual compute used
    let simulation_result = client.as_ref().simulate_transaction(&snub_tx).await?;
    if let Some(err) = simulation_result.value.err {
        println!("Error: {}", err);
        if let Some(logs) = simulation_result.value.logs {
            for log in logs {
                println!("Log: {}", log);
            }
        }
    }
    let actual_compute_used = simulation_result.value.units_consumed.unwrap_or(200000);

    let final_compute_budget = (actual_compute_used as f32 * compute_multiplier) as u32;
    Ok((
        compute_budget_instruction(final_compute_budget),
        final_compute_budget,
    ))
}

// Returns the instructions and the total fee in lamports
pub async fn auto_compute_limit_and_price<C: AsRef<RpcClient>, T: Signers + ?Sized>(
    client: &C,
    instructions: Vec<Instruction>,
    signers: &T,
    compute_multiplier: f32,
    payer: Option<&Pubkey>,
    blockhash: Option<solana_program::hash::Hash>,
) -> Result<(Vec<Instruction>, u64), Error> {
    let mut updated_instructions = instructions.clone();

    // Compute budget instruction
    let (compute_budget_ix, compute_limit) = compute_budget_for_instructions(
        client,
        instructions.clone(),
        signers,
        compute_multiplier,
        payer,
        blockhash,
    )
    .await?;

    // Compute price instruction
    let accounts: Vec<Pubkey> = instructions
        .iter()
        .flat_map(|i| i.accounts.iter().map(|a| a.pubkey))
        .unique()
        .collect();
    let (compute_price_ix, priority_fee) =
        compute_price_instruction_for_accounts(client, &accounts).await?;

    // Replace or insert compute budget instruction
    if let Some(pos) = updated_instructions
        .iter()
        .position(|ix| ix.program_id == solana_sdk::compute_budget::id())
    {
        updated_instructions[pos] = compute_budget_ix; // Replace existing
    } else {
        updated_instructions.insert(0, compute_budget_ix); // Insert at the beginning
    }

    // Replace or insert compute price instruction
    if let Some(pos) = updated_instructions
        .iter()
        .position(|ix| ix.program_id == solana_sdk::compute_budget::id())
    {
        updated_instructions[pos + 1] = compute_price_ix; // Replace existing
    } else {
        updated_instructions.insert(1, compute_price_ix); // Insert after compute budget
    }

    Ok((
        updated_instructions,
        // Get the actual total fee in lamports
        5000 + (priority_fee * (compute_limit as u64)) / 1000000,
    ))
}
