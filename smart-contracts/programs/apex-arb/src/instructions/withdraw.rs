use anchor_lang::prelude::*;
use crate::{errors::ApexError, state::ApexConfig};

#[derive(Accounts)]
pub struct WithdrawProfit<'info> {
    #[account(
        seeds = [b"apex-config"],
        bump  = config.bump,
        has_one = owner @ ApexError::Unauthorized,
    )]
    pub config:   Account<'info, ApexConfig>,
    pub owner:    Signer<'info>,

    #[account(mut, constraint = treasury.key() == config.treasury)]
    pub treasury: SystemAccount<'info>,

    #[account(mut)]
    pub destination: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WithdrawProfit>, amount: u64) -> Result<()> {
    let treasury = &ctx.accounts.treasury;
    require!(treasury.lamports() >= amount, ApexError::InsufficientOutput);

    anchor_lang::system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.treasury.to_account_info(),
                to:   ctx.accounts.destination.to_account_info(),
            },
        ),
        amount,
    )?;

    msg!("Withdrew {} lamports from treasury", amount);
    Ok(())
}
