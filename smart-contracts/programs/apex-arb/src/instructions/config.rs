use anchor_lang::prelude::*;
use crate::{errors::ApexError, state::ApexConfig};

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [b"apex-config"],
        bump  = config.bump,
        has_one = owner @ ApexError::Unauthorized,
    )]
    pub config: Account<'info, ApexConfig>,
    pub owner:  Signer<'info>,
}

pub fn handler(
    ctx:             Context<UpdateConfig>,
    min_profit:      u64,
    max_slippage_bps: u16,
    enabled:         bool,
) -> Result<()> {
    let config = &mut ctx.accounts.config;
    config.min_profit_lamports = min_profit;
    config.max_slippage_bps    = max_slippage_bps;
    config.enabled             = enabled;
    msg!("Config updated: min_profit={} slippage_bps={} enabled={}", min_profit, max_slippage_bps, enabled);
    Ok(())
}
