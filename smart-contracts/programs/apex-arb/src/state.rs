use anchor_lang::prelude::*;

#[account]
#[derive(Default)]
pub struct ApexConfig {
    pub owner:            Pubkey,
    pub treasury:         Pubkey,
    pub min_profit_lamports: u64,
    pub max_slippage_bps:    u16,
    pub enabled:             bool,
    pub total_arb_count:     u64,
    pub total_profit:        u64,
    pub bump:                u8,
}

impl ApexConfig {
    pub const LEN: usize = 8   // discriminator
        + 32   // owner
        + 32   // treasury
        + 8    // min_profit_lamports
        + 2    // max_slippage_bps
        + 1    // enabled
        + 8    // total_arb_count
        + 8    // total_profit
        + 1;   // bump
}

#[account]
pub struct ArbReceipt {
    pub executor:         Pubkey,
    pub input_mint:       Pubkey,
    pub output_mint:      Pubkey,
    pub input_amount:     u64,
    pub output_amount:    u64,
    pub profit:           u64,
    pub executed_at:      i64,
    pub slot:             u64,
    pub hop_count:        u8,
}

impl ArbReceipt {
    pub const LEN: usize = 8  // discriminator
        + 32 + 32 + 32        // pubkeys
        + 8 + 8 + 8 + 8 + 8  // amounts + slot + time
        + 1;                  // hop_count
}
