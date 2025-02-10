use anchor_lang::prelude::*;

use crate::state::TuktukConfigV0;

pub const TESTING: bool = std::option_env!("TESTING").is_some();

pub static APPROVER: Pubkey = pubkey!("hprdnjkbziK8NqhThmAn5Gu4XqrBbctX8du4PfJdgvW");

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct InitializeTuktukConfigArgsV0 {
    pub min_deposit: u64,
}

#[derive(Accounts)]
pub struct InitializeTuktukConfigV0<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
      constraint = TESTING || approver.key() == APPROVER
    )]
    pub approver: Signer<'info>,
    /// CHECK: Is getting set by signer
    pub authority: UncheckedAccount<'info>,
    #[account(
      init,
      payer = payer,
      seeds = ["tuktuk_config".as_bytes()],
      bump,
      space = TuktukConfigV0::INIT_SPACE + 60,
    )]
    pub tuktuk_config: Account<'info, TuktukConfigV0>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<InitializeTuktukConfigV0>,
    args: InitializeTuktukConfigArgsV0,
) -> Result<()> {
    ctx.accounts.tuktuk_config.set_inner(TuktukConfigV0 {
        authority: ctx.accounts.authority.key(),
        bump_seed: ctx.bumps.tuktuk_config,
        min_task_queue_id: 0,
        next_task_queue_id: 0,
        min_deposit: args.min_deposit,
    });
    Ok(())
}
