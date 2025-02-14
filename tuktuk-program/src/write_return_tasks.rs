use anchor_lang::{
    prelude::*,
    solana_program::entrypoint::MAX_PERMITTED_DATA_INCREASE,
    system_program::{self, transfer, Transfer},
};

use crate::TaskReturnV0;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct TasksAccountHeaderV0 {
    pub num_tasks: u32,
}

pub struct WriteReturnTasksArgs<'info, I: Iterator<Item = TaskReturnV0>> {
    pub program_id: Pubkey,
    pub payer_info: PayerInfo<'info>,
    pub accounts: Vec<AccountWithSeeds<'info>>,
    pub tasks: I,
    pub system_program: AccountInfo<'info>,
}

pub enum PayerInfo<'info> {
    PdaPayer(AccountInfo<'info>),
    SystemPayer {
        account_info: AccountInfo<'info>,
        seeds: Vec<Vec<u8>>,
    },
    Signer(AccountInfo<'info>),
}

#[derive(Clone)]
pub struct AccountWithSeeds<'info> {
    pub account: AccountInfo<'info>,
    pub seeds: Vec<Vec<u8>>,
}

pub struct WriteReturnTasksReturn {
    pub used_accounts: Vec<Pubkey>,
    pub total_tasks: u32,
}

// Fills accounts with tasks up to the maximum length of 10kb, then moves on to the next account until it is out of tasks.
// It should return a vector of the pubkeys of the accounts it used.
// Note that tuktuk does not clean up these accounts, but you can reuse them with this method (it will overwrite)
pub fn write_return_tasks<I>(args: WriteReturnTasksArgs<'_, I>) -> Result<WriteReturnTasksReturn>
where
    I: Iterator<Item = TaskReturnV0>,
{
    let WriteReturnTasksArgs {
        program_id,
        payer_info,
        accounts,
        mut tasks,
        system_program,
    } = args;
    let mut used_accounts = Vec::with_capacity(accounts.len());
    let mut original_sizes = Vec::with_capacity(accounts.len());

    // Get the first task outside the loop to check if we have any tasks
    let mut current_task = match tasks.next() {
        Some(task) => task,
        None => {
            return Ok(WriteReturnTasksReturn {
                used_accounts,
                total_tasks: 0,
            })
        }
    };

    let mut total_tasks = 0;
    for AccountWithSeeds { account, seeds } in accounts.iter() {
        // Store original size before any reallocation
        original_sizes.push(account.data_len());

        let mut header = TasksAccountHeaderV0 { num_tasks: 0 };
        let header_size = header.try_to_vec()?.len();
        let mut total_size = header_size;

        msg!("Assigning account {} and allocating space", account.key());
        if account.owner == &system_program::ID {
            // Assign account to our program
            let seeds_refs: Vec<&[u8]> = seeds.iter().map(|s| s.as_slice()).collect();
            let seeds_slice: &[&[u8]] = seeds_refs.as_slice();
            system_program::assign(
                CpiContext::new_with_signer(
                    system_program.to_account_info(),
                    system_program::Assign {
                        account_to_assign: account.to_account_info(),
                    },
                    &[seeds_slice],
                ),
                &program_id,
            )?;
        }
        account.realloc(MAX_PERMITTED_DATA_INCREASE, false)?;
        let mut data = account.data.borrow_mut();

        // Write tasks directly after header
        let mut offset = header_size;
        let mut num_tasks = 0;

        loop {
            let task_bytes = current_task.try_to_vec()?;
            if offset + task_bytes.len() > MAX_PERMITTED_DATA_INCREASE {
                break; // This task will be handled by the next account
            }

            data[offset..offset + task_bytes.len()].copy_from_slice(&task_bytes);
            offset += task_bytes.len();
            total_size += task_bytes.len();
            num_tasks += 1;
            total_tasks += 1;

            // Get next task
            match tasks.next() {
                Some(task) => current_task = task,
                None => {
                    break;
                }
            }
        }

        if num_tasks > 0 {
            header.num_tasks = num_tasks;

            // Write header
            let header_bytes = header.try_to_vec()?;
            data[..header_size].copy_from_slice(&header_bytes);
            drop(data);

            // Resize account to actual size
            account.realloc(total_size, false)?;
            let rent = Rent::get()?.minimum_balance(total_size);
            let current_lamports = account.lamports();
            let rent_to_pay = rent.saturating_sub(current_lamports);
            if rent_to_pay > 0 {
                match &payer_info {
                    PayerInfo::PdaPayer(account_info) => {
                        if account_info.lamports()
                            - Rent::get()?.minimum_balance(account_info.data_len())
                            < rent_to_pay
                        {
                            // Reset all account sizes on error
                            for (account, original_size) in
                                accounts.iter().zip(original_sizes.iter())
                            {
                                account.account.realloc(*original_size, false)?;
                            }
                            return Err(error!(ErrorCode::ConstraintRentExempt));
                        }
                        account_info.sub_lamports(rent_to_pay)?;
                        account.add_lamports(rent_to_pay)?;
                    }
                    PayerInfo::SystemPayer {
                        account_info,
                        seeds,
                    } => {
                        let payer_seeds_refs: Vec<&[u8]> =
                            seeds.iter().map(|s| s.as_slice()).collect();

                        if account_info.lamports()
                            - Rent::get()?.minimum_balance(account_info.data_len())
                            < rent_to_pay
                        {
                            // Reset all account sizes on error
                            for (account, original_size) in
                                accounts.iter().zip(original_sizes.iter())
                            {
                                account.account.realloc(*original_size, false)?;
                            }
                            return Err(error!(ErrorCode::ConstraintRentExempt));
                        }
                        transfer(
                            CpiContext::new_with_signer(
                                system_program.clone(),
                                Transfer {
                                    from: account_info.clone(),
                                    to: account.clone(),
                                },
                                &[payer_seeds_refs.as_slice()],
                            ),
                            rent_to_pay,
                        )?;
                    }
                    PayerInfo::Signer(account_info) => {
                        if account_info.lamports()
                            - Rent::get()?.minimum_balance(account_info.data_len())
                            < rent_to_pay
                        {
                            // Reset all account sizes on error
                            for (account, original_size) in
                                accounts.iter().zip(original_sizes.iter())
                            {
                                account.account.realloc(*original_size, false)?;
                            }
                            return Err(error!(ErrorCode::ConstraintRentExempt));
                        }
                        transfer(
                            CpiContext::new(
                                system_program.clone(),
                                Transfer {
                                    from: account_info.clone(),
                                    to: account.clone(),
                                },
                            ),
                            rent_to_pay,
                        )?;
                    }
                }
            }

            used_accounts.push(*account.key);
        } else {
            drop(data);
            account.realloc(0, false)?;
        }

        // If we've processed all tasks, we can exit
        if num_tasks == 0 || tasks.next().is_none() {
            break;
        }
    }

    // Check if we still have unprocessed tasks
    if tasks.next().is_some() {
        return Err(error!(ErrorCode::ConstraintRaw));
    }

    Ok(WriteReturnTasksReturn {
        used_accounts,
        total_tasks,
    })
}
