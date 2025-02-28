use std::{collections::HashSet, result::Result, str::FromStr};

use anchor_lang::{prelude::AccountMeta, InstructionData, ToAccountMetas};
use base64::{prelude::BASE64_STANDARD, Engine};
use bytemuck::{bytes_of, Pod, Zeroable};
use serde::Deserialize;
use solana_client::client_error::reqwest;
use solana_sdk::{
    address_lookup_table::{state::AddressLookupTable, AddressLookupTableAccount},
    ed25519_instruction::{DATA_START, PUBKEY_SERIALIZED_SIZE, SIGNATURE_SERIALIZED_SIZE},
    ed25519_program,
    instruction::Instruction,
    pubkey::Pubkey,
};
use tuktuk_program::{tuktuk, TaskQueueV0, TaskV0, TransactionSourceV0};

use crate::{client::GetAnchorAccount, error::Error};

pub fn next_available_task_ids_excluding_in_progress(
    capacity: u16,
    task_bitmap: &[u8],
    n: u8,
    in_progress_task_ids: &HashSet<u16>,
    start_idx: usize,
) -> Result<Vec<u16>, Error> {
    if n == 0 {
        return Ok(vec![]);
    }

    let mut available_task_ids = Vec::new();
    for offset in 0..task_bitmap.len() {
        let byte_idx = (start_idx + offset) % task_bitmap.len();
        let byte = task_bitmap[byte_idx];
        if byte != 0xff {
            // If byte is not all 1s
            for bit_idx in 0..8 {
                let id = (byte_idx * 8 + bit_idx) as u16;
                if id < capacity
                    && byte & (1 << bit_idx) == 0
                    && !in_progress_task_ids.contains(&id)
                {
                    available_task_ids.push(id);
                    if available_task_ids.len() == n as usize {
                        return Ok(available_task_ids);
                    }
                }
            }
        }
    }

    // Return error if we couldn't find enough free task IDs
    if available_task_ids.len() < n as usize && n > 0 {
        Err(Error::NotEnoughFreeTasks)
    } else {
        Ok(available_task_ids)
    }
}

pub struct RunTaskResult {
    pub instructions: Vec<Instruction>,
    pub free_task_ids: Vec<u16>,
    pub lookup_tables: Vec<AddressLookupTableAccount>,
}

pub async fn run_ix_with_free_tasks(
    task_key: Pubkey,
    task: &TaskV0,
    payer: Pubkey,
    next_available: Vec<u16>,
    lookup_tables: Vec<AddressLookupTableAccount>,
) -> Result<Option<RunTaskResult>, Error> {
    let transaction = &task.transaction;

    let free_tasks = next_available
        .iter()
        .map(|id| AccountMeta {
            pubkey: Pubkey::find_program_address(
                &[b"task", task.task_queue.as_ref(), &id.to_le_bytes()],
                &tuktuk_program::tuktuk::ID,
            )
            .0,
            is_signer: false,
            is_writable: true,
        })
        .collect::<Vec<_>>();

    match transaction {
        TransactionSourceV0::CompiledV0(transaction) => {
            let remaining_accounts: Vec<AccountMeta> = transaction
                .accounts
                .iter()
                .enumerate()
                .map(|(index, acc)| {
                    let is_writable = index < transaction.num_rw_signers as usize
                        || (index
                            >= (transaction.num_rw_signers + transaction.num_ro_signers) as usize
                            && index
                                < (transaction.num_rw_signers
                                    + transaction.num_ro_signers
                                    + transaction.num_rw)
                                    as usize);

                    AccountMeta {
                        pubkey: *acc,
                        is_signer: false,
                        is_writable,
                    }
                })
                .collect();

            let ix_accounts = tuktuk_program::client::accounts::RunTaskV0 {
                rent_refund: task.rent_refund,
                task_queue: task.task_queue,
                task: task_key,
                crank_turner: payer,
                system_program: solana_sdk::system_program::id(),
                sysvar_instructions: solana_sdk::sysvar::instructions::id(),
            };

            let all_accounts = [
                ix_accounts.to_account_metas(None),
                remaining_accounts,
                free_tasks,
            ]
            .concat();

            Ok(Some(RunTaskResult {
                instructions: vec![Instruction {
                    program_id: tuktuk_program::tuktuk::ID,
                    accounts: all_accounts,
                    data: tuktuk::client::args::RunTaskV0 {
                        args: tuktuk_program::types::RunTaskArgsV0 {
                            free_task_ids: next_available.clone(),
                        },
                    }
                    .data(),
                }],
                lookup_tables,
                free_task_ids: next_available,
            }))
        }
        TransactionSourceV0::RemoteV0 { signer, url } => {
            // Fetch the remote transaction
            let remote_transaction =
                fetch_remote_transaction(&task.task_queue, &task_key, &task.queued_at, url).await?;
            let message = remote_transaction.transaction;
            let signature = remote_transaction.signature;
            let mut instruction_data = Vec::with_capacity(
                DATA_START
                    .saturating_add(SIGNATURE_SERIALIZED_SIZE)
                    .saturating_add(PUBKEY_SERIALIZED_SIZE)
                    .saturating_add(message.len()),
            );

            let num_signatures: u8 = 1;
            let public_key_offset = DATA_START;
            let signature_offset = public_key_offset.saturating_add(PUBKEY_SERIALIZED_SIZE);
            let message_data_offset = signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);

            // add padding byte so that offset structure is aligned
            instruction_data.extend_from_slice(bytes_of(&[num_signatures, 0]));

            let offsets = Ed25519SignatureOffsets {
                signature_offset: signature_offset as u16,
                signature_instruction_index: u16::MAX,
                public_key_offset: public_key_offset as u16,
                public_key_instruction_index: u16::MAX,
                message_data_offset: message_data_offset as u16,
                message_data_size: message.len() as u16,
                message_instruction_index: u16::MAX,
            };

            instruction_data.extend_from_slice(bytes_of(&offsets));
            instruction_data.extend_from_slice(signer.to_bytes().as_ref());
            instruction_data.extend_from_slice(&signature);
            instruction_data.extend_from_slice(&message);

            Ok(Some(RunTaskResult {
                lookup_tables,
                instructions: vec![
                    Instruction {
                        program_id: ed25519_program::ID,
                        accounts: vec![],
                        data: instruction_data,
                    },
                    Instruction {
                        program_id: tuktuk_program::tuktuk::ID,
                        accounts: [
                            tuktuk::client::accounts::RunTaskV0 {
                                rent_refund: task.rent_refund,
                                task_queue: task.task_queue,
                                task: task_key,
                                crank_turner: payer,
                                system_program: solana_sdk::system_program::id(),
                                sysvar_instructions: solana_sdk::sysvar::instructions::id(),
                            }
                            .to_account_metas(None),
                            remote_transaction.remaining_accounts,
                            free_tasks,
                        ]
                        .concat(),
                        data: tuktuk::client::args::RunTaskV0 {
                            args: tuktuk_program::types::RunTaskArgsV0 {
                                free_task_ids: next_available.clone(),
                            },
                        }
                        .data(),
                    },
                ],
                free_task_ids: next_available,
            }))
        }
    }
}

pub async fn run_ix(
    client: &impl GetAnchorAccount,
    task_key: Pubkey,
    payer: Pubkey,
    in_progress_task_ids: &HashSet<u16>,
) -> Result<Option<RunTaskResult>, Error> {
    let task: TaskV0 = client
        .anchor_account(&task_key)
        .await?
        .ok_or_else(|| Error::AccountNotFound)?;

    let task_queue: TaskQueueV0 = client
        .anchor_account(&task.task_queue)
        .await?
        .ok_or_else(|| Error::AccountNotFound)?;

    // Get next available task IDs excluding in-progress ones
    let next_available = next_available_task_ids_excluding_in_progress(
        task_queue.capacity,
        &task_queue.task_bitmap,
        task.free_tasks,
        in_progress_task_ids,
        rand::random_range(0..task_queue.task_bitmap.len()),
    )?;

    let lookup_tables = client
        .accounts(&task_queue.lookup_tables)
        .await?
        .into_iter()
        .filter_map(|(addr, raw)| {
            raw.map(|acc| {
                let lut = AddressLookupTable::deserialize(&acc.data).map_err(Error::from)?;
                Ok::<AddressLookupTableAccount, Error>(AddressLookupTableAccount {
                    key: addr,
                    addresses: lut.addresses.to_vec(),
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    run_ix_with_free_tasks(task_key, &task, payer, next_available, lookup_tables).await
}

#[derive(Default, Debug, Copy, Clone, Zeroable, Pod, Eq, PartialEq)]
#[repr(C)]
pub struct Ed25519SignatureOffsets {
    signature_offset: u16,             // offset to ed25519 signature of 64 bytes
    signature_instruction_index: u16,  // instruction index to find signature
    public_key_offset: u16,            // offset to public key of 32 bytes
    public_key_instruction_index: u16, // instruction index to find public key
    message_data_offset: u16,          // offset to start of message data
    message_data_size: u16,            // size of message data
    message_instruction_index: u16,    // index of instruction data to get message data
}

#[derive(Deserialize)]
struct RemoteAccountMeta {
    pubkey: String,
    is_writable: bool,
    is_signer: bool,
}

#[derive(Deserialize)]
struct RemoteResponse {
    transaction: String,
    remaining_accounts: Vec<RemoteAccountMeta>,
    signature: String,
}

struct FetchedRemoteResponse {
    transaction: Vec<u8>,
    remaining_accounts: Vec<AccountMeta>,
    signature: Vec<u8>,
}

async fn fetch_remote_transaction(
    task_queue: &Pubkey,
    task: &Pubkey,
    task_queued_at: &i64,
    url: &str,
) -> Result<FetchedRemoteResponse, Error> {
    let client = reqwest::Client::new();
    println!("Fetching remote transaction for task: {}", url);
    let response = client
        .post(url)
        .json(&serde_json::json!({
            "task": task.to_string(),
            "task_queue": task_queue.to_string(),
            "task_queued_at": task_queued_at.to_string()
        }))
        .send()
        .await?;

    let json: RemoteResponse = response.json().await?;

    let remaining_accounts = json
        .remaining_accounts
        .into_iter()
        .map(|acc| {
            Ok(AccountMeta {
                pubkey: Pubkey::from_str(&acc.pubkey).map_err(Error::from)?,
                is_writable: acc.is_writable,
                is_signer: acc.is_signer,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let transaction_bytes = BASE64_STANDARD
        .decode(&json.transaction)
        .map_err(Error::from)?;
    let signature_bytes = BASE64_STANDARD
        .decode(&json.signature)
        .map_err(Error::from)?;

    Ok(FetchedRemoteResponse {
        transaction: transaction_bytes,
        remaining_accounts,
        signature: signature_bytes,
    })
}
