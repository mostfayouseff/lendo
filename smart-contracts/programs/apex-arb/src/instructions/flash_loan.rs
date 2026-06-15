use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

use crate::{errors::ApexError, state::{ApexConfig, ArbReceipt}};

/// Flash loan arbitrage instruction.
///
/// Sequence executed atomically in a single transaction:
///   1. CPI to flash loan provider (Solend / MarginFi / Kamino) — borrow
///   2. CPI to Jupiter V6 — swap across optimal route
///   3. Assert output ≥ repay_amount + min_profit
///   4. CPI to flash loan provider — repay
///   5. Transfer profit to treasury
///   6. Write ArbReceipt on-chain
#[derive(Accounts)]
#[instruction(borrow_amount: u64)]
pub struct FlashLoanArb<'info> {
    #[account(
        seeds  = [b"apex-config"],
        bump   = config.bump,
        constraint = config.enabled @ ApexError::ProtocolPaused,
    )]
    pub config: Account<'info, ApexConfig>,

    #[account(
        init,
        payer  = executor,
        space  = ArbReceipt::LEN,
        seeds  = [b"receipt", executor.key().as_ref(), &Clock::get()?.slot.to_le_bytes()],
        bump,
    )]
    pub receipt: Account<'info, ArbReceipt>,

    #[account(mut)]
    pub executor: Signer<'info>,

    #[account(mut, constraint = executor_token_account.owner == executor.key())]
    pub executor_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub treasury: SystemAccount<'info>,

    pub token_program:  Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx:           Context<FlashLoanArb>,
    borrow_amount: u64,
    min_profit:    u64,
    _route_data:   Vec<u8>,
) -> Result<()> {
    let config = &ctx.accounts.config;

    require!(min_profit >= config.min_profit_lamports, ApexError::NotProfitable);
    require!(borrow_amount > 0, ApexError::InvalidRouteData);

    let clock = Clock::get()?;

    msg!(
        "Flash loan arb: borrow={} min_profit={} executor={}",
        borrow_amount, min_profit, ctx.accounts.executor.key()
    );

    // ── Step 1: Borrow via CPI (provider CPI would be inserted here) ──────────
    // In production: invoke flash_borrow on Solend/MarginFi/Kamino reserve.

    // ── Step 2: Swap via Jupiter V6 CPI ──────────────────────────────────────
    // In production: invoke Jupiter V6 program with route_data as instruction data.

    // ── Step 3: Assert profitability ──────────────────────────────────────────
    let output_amount = borrow_amount; // placeholder — real value comes from CPI output
    let repay_amount  = borrow_amount + borrow_amount / 1000; // example 0.1% fee
    require!(
        output_amount >= repay_amount.checked_add(min_profit).ok_or(ApexError::Overflow)?,
        ApexError::NotProfitable
    );

    let profit = output_amount.checked_sub(repay_amount).ok_or(ApexError::Overflow)?;

    // ── Step 4: Repay flash loan via CPI ──────────────────────────────────────

    // ── Step 5: Write receipt ─────────────────────────────────────────────────
    let receipt = &mut ctx.accounts.receipt;
    receipt.executor      = ctx.accounts.executor.key();
    receipt.input_amount  = borrow_amount;
    receipt.output_amount = output_amount;
    receipt.profit        = profit;
    receipt.executed_at   = clock.unix_timestamp;
    receipt.slot          = clock.slot;
    receipt.hop_count     = 1;

    msg!("Arb complete: profit={} slot={}", profit, clock.slot);
    Ok(())
}
