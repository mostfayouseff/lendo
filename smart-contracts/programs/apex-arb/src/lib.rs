use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

declare_id!("ApexArb1111111111111111111111111111111111111");

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

#[program]
pub mod apex_arb {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        instructions::initialize::handler(ctx)
    }

    pub fn flash_loan_arb(
        ctx:           Context<FlashLoanArb>,
        borrow_amount: u64,
        min_profit:    u64,
        route_data:    Vec<u8>,
    ) -> Result<()> {
        instructions::flash_loan::handler(ctx, borrow_amount, min_profit, route_data)
    }

    pub fn update_config(
        ctx:            Context<UpdateConfig>,
        min_profit:     u64,
        max_slippage_bps: u16,
        enabled:        bool,
    ) -> Result<()> {
        instructions::config::handler(ctx, min_profit, max_slippage_bps, enabled)
    }

    pub fn withdraw_profit(ctx: Context<WithdrawProfit>, amount: u64) -> Result<()> {
        instructions::withdraw::handler(ctx, amount)
    }
}
