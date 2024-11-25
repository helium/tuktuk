use anchor_lang::{
    prelude::*,
    solana_program::{self, instruction::Instruction},
};
use anchor_spl::token::{transfer, Token, Transfer};

use crate::{
    error::ErrorCode,
    state::{TaskQueueV0, TaskV0},
    task_queue_seeds,
};

#[derive(Accounts)]
pub struct RunTaskV0<'info> {
    /// CHECK: Via has one
    #[account(mut)]
    pub rent_refund: AccountInfo<'info>,
    #[account(
        mut,
        has_one = rewards_source,
    )]
    pub task_queue: Account<'info, TaskQueueV0>,
    #[account(
        mut,
        has_one = task_queue,
        has_one = rent_refund,
        close = rent_refund,
        constraint = task.trigger.is_active()? @ ErrorCode::TaskNotReady,
    )]
    pub task: Box<Account<'info, TaskV0>>,
    /// CHECK: Via CPI
    #[account(mut)]
    pub rewards_source: AccountInfo<'info>,
    /// CHECK: Via CPI
    #[account(mut)]
    pub rewards_destination: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<RunTaskV0>) -> Result<()> {
    ctx.accounts
        .task_queue
        .set_task_exists(ctx.accounts.task.id as usize, false);
    let transaction = &ctx.accounts.task.transaction;

    let prefix: Vec<&[u8]> = vec![b"custom", ctx.accounts.task.task_queue.as_ref()];
    // Need to convert to &[&[u8]] because invoke_signed expects that
    let signers_inner_u8: Vec<Vec<&[u8]>> = transaction
        .signer_seeds
        .iter()
        .map(|s| {
            let mut clone = prefix.clone();
            clone.extend(s.iter().map(|v| v.as_slice()).collect::<Vec<&[u8]>>());

            clone
        })
        .collect();
    let signers = signers_inner_u8
        .iter()
        .map(|s| s.as_slice())
        .collect::<Vec<&[&[u8]]>>();

    let signer_addresses = signers
        .iter()
        .map(|s| Pubkey::create_program_address(s, ctx.program_id).unwrap())
        .collect::<std::collections::HashSet<Pubkey>>();

    // Validate txn
    for (index, account) in ctx.remaining_accounts.iter().enumerate() {
        let signers_end = (transaction.num_ro_signers + transaction.num_rw_signers) as usize;
        // It is okay if an account not labeled as a signer is a signer.
        // For example, if an account being passed is a fee payer
        if index < signers_end {
            require!(
                account.is_signer || signer_addresses.contains(&account.key()),
                ErrorCode::InvalidSigner,
            );
        }

        let is_writable = index < transaction.num_rw as usize
            || (index >= signers_end && index < (signers_end + transaction.num_rw as usize));
        // While it would be nice to validate non-writable accounts aren't writable,
        // this is not possible. We can't tell who the tx fee payer is, so they may be writable
        // because of that. Or they may be the refund target.
        if is_writable {
            require!(account.is_writable, ErrorCode::InvalidWritable);
        }

        require_eq!(
            *account.key,
            transaction.accounts[index],
            ErrorCode::InvalidAccount
        );
    }
    for ix in &transaction.instructions {
        let mut accounts = Vec::new();
        let mut account_infos = Vec::new();
        for i in &ix.accounts {
            let acct = ctx.remaining_accounts[*i as usize].clone();
            accounts.push(acct.clone());
            account_infos.push(AccountMeta {
                pubkey: acct.key(),
                is_signer: acct.is_signer || signer_addresses.contains(&acct.key()),
                is_writable: acct.is_writable,
            })
        }
        solana_program::program::invoke_signed(
            &Instruction {
                program_id: *ctx.remaining_accounts[ix.program_id_index as usize].key,
                accounts: account_infos,
                data: ix.data.clone(),
            },
            accounts.as_slice(),
            &signers,
        )?;
    }

    transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.rewards_source.to_account_info(),
                to: ctx.accounts.rewards_destination.to_account_info(),
                authority: ctx.accounts.task_queue.to_account_info(),
            },
            &[task_queue_seeds!(ctx.accounts.task_queue)],
        ),
        ctx.accounts.task.crank_reward,
    )?;

    Ok(())
}
