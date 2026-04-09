// =============================================================================
// INGRESS CRATE — Live data ingress, opportunity detection, swap instructions
//
// Ingress streams:
//   PRIMARY:   Helius WebSocket (transactionSubscribe + logsSubscribe)
//   FALLBACK:  Alchemy WebSocket (logsSubscribe)
//   PRICE:     Multi-source Jupiter price monitor (self-healing)
//
// Jupiter Swap V2 API (https://api.jup.ag/swap/v2):
//   GET /order  → detection.rs  — opportunity detection only
//   GET /build  → swap_v2.rs   — atomic swap instructions + ALT resolution
//
// FORBIDDEN endpoints:
//   /swap/v1/*  |  ultra/v1/*  |  /swap-instructions  |  lite-api.jup.ag
//   POST /execute
// =============================================================================

pub mod detection;
pub mod filter;
pub mod helius_ws;
pub mod jupiter;
pub mod shredstream;
pub mod swap_v2;
pub mod yellowstone;

pub use detection::{
    build_swap_v2_client, detect_opportunity, OpportunityQuote, OrderRoutePlanStep,
    OrderSwapInfo, SWAP_V2_BASE, MAX_PRICE_IMPACT_PCT,
};
pub use filter::{EbpfFilter, FilterRule};
pub use helius_ws::{AlchemyTransactionStream, HeliusTransactionStream, DEX_PROGRAMS};
pub use jupiter::{build_token_info_map, JupiterMonitor, KnownToken, TokenInfo, TOKENS};
pub use shredstream::{MockShredStream, ShredEvent};
pub use swap_v2::{
    build_tip_instruction, get_build_instructions, BuildInstructions,
    V2AccountMeta, V2Instruction,
};
pub use yellowstone::{MockYellowstoneStream, SlotUpdate};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IngressError {
    #[error("Malformed packet: {0}")]
    MalformedPacket(String),
    #[error("Packet too large: {size} bytes (max {max})")]
    PacketTooLarge { size: usize, max: usize },
    #[error("Channel closed")]
    ChannelClosed,
    #[error("Filter rejected packet")]
    FilterRejected,
}
