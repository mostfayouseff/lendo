// =============================================================================
// SECURITY AUDIT CHECKLIST — common/src/types.rs
// [✓] All profit/fee calculations use Decimal (no floating-point precision bugs)
// [✓] PriceMatrix uses f64 in hot-path data; Decimal stays in MarketEdge
// [✓] No Clone-derived types expose secrets
// [✓] TokenMint is opaque (newtype), prevents mixing up mint addresses
// =============================================================================

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Opaque newtype for a token mint address (32-byte base58-encoded public key).
/// Using a newtype prevents accidentally swapping source/dest mints.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenMint(pub [u8; 32]);

impl fmt::Display for TokenMint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl TokenMint {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[cfg(test)]
    #[must_use]
    pub fn zero() -> Self {
        Self([0u8; 32])
    }
}

/// Supported DEX identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dex {
    Raydium,
    Orca,
    Meteora,
    Phoenix,
    JupiterV6,
}

impl fmt::Display for Dex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raydium => write!(f, "Raydium"),
            Self::Orca => write!(f, "Orca"),
            Self::Meteora => write!(f, "Meteora"),
            Self::Phoenix => write!(f, "Phoenix"),
            Self::JupiterV6 => write!(f, "JupiterV6"),
        }
    }
}

/// A directed weighted edge between two tokens on a specific DEX.
/// `log_weight` stored as Decimal for precision in P&L accounting.
/// The hot-path matrix converts to f64 once at build time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEdge {
    pub from: TokenMint,
    pub to: TokenMint,
    pub dex: Dex,
    /// ln(out_amount / in_amount) — negative ↔ profitable direction
    pub log_weight: Decimal,
    /// Absolute liquidity in lamports; used for position sizing
    pub liquidity_lamports: u64,
    /// Slot when this edge was last refreshed
    pub slot: u64,
}

/// A full arbitrage path (sequence of edges forming a cycle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbPath {
    pub edges: Vec<MarketEdge>,
    pub expected_profit_lamports: u64,
    pub gnn_confidence: f32,
    pub rich_color: RichColor,
}

/// RICH color-coding state for a path/cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RichColor {
    White,
    Gray,
    Black,
}

/// N×N price matrix — hot path uses f64 natively.
/// Entry [i][j] = ln(exchange_rate) for token i → token j.
/// f64::INFINITY = no edge between this pair.
///
/// Layout: row-major flat Vec<f64> for maximum cache locality.
/// The AVX2 inner loop in rich_engine reads this directly with no conversion.
#[derive(Debug, Clone)]
pub struct PriceMatrix {
    pub n: usize,
    /// Row-major f64 weights; INFINITY = no edge
    pub data: Vec<f64>,
    pub tokens: Vec<TokenMint>,
}

impl PriceMatrix {
    /// Construct a matrix pre-filled with INFINITY (no edges).
    #[must_use]
    pub fn new(tokens: Vec<TokenMint>) -> Self {
        let n = tokens.len();
        let capacity = n.saturating_mul(n);
        Self {
            n,
            data: vec![f64::INFINITY; capacity],
            tokens,
        }
    }

    /// Safe indexed get; returns None on out-of-bounds.
    #[inline]
    #[must_use]
    pub fn get(&self, row: usize, col: usize) -> Option<f64> {
        let idx = row.checked_mul(self.n)?.checked_add(col)?;
        self.data.get(idx).copied()
    }

    /// Safe indexed set. Returns false on out-of-bounds.
    #[inline]
    pub fn set(&mut self, row: usize, col: usize, val: f64) -> bool {
        if let Some(idx) = row.checked_mul(self.n).and_then(|r| r.checked_add(col)) {
            if let Some(slot) = self.data.get_mut(idx) {
                *slot = val;
                return true;
            }
        }
        false
    }
}

/// Result of a submitted Solana transaction / Jito bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResult {
    pub signature: Vec<u8>,
    pub slot: u64,
    pub profit_lamports: i64,
    pub success: bool,
    pub error: Option<String>,
}

// ─── hex helper ──────────────────────────────────────────────────────────────
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}
