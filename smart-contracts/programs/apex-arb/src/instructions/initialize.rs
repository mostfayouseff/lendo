use anchor_lang::prelude::*;
use crate::state::ApexConfig;

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer  = owner,
        space  = ApexConfig::LEN,
        seeds  = [b"apex-config"],
        bump,
    )]
    pub config: Account<'info, ApexConfig>,

    #[account(mut)]
    pub owner: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<Initialize>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    config.owner               = ctx.accounts.owner.key();
    config.treasury            = ctx.accounts.owner.key();
    config.min_profit_lamports = 10_000;
    config.max_slippage_bps    = 50;
    config.enabled             = true;
    config.bump                = ctx.bumps.config;
    msg!("Apex arbitrage program initialized, owner={}", config.owner);
    Ok(())
}
