// =============================================================================
// INGRESS CRATE — Live data ingress, opportunity detection, swap instructions
//
// Ingress streams:
//   PRIMARY:   Helius WebSocket (transactionSubscribe + logsSubscribe)
//   FALLBACK:  Alchemy WebSocket (logsSubscribe)
//   PRICE:     Multi-source Jupiter price monitor (self-healing)
//
// Jupiter Ultra API (https://api.jup.ag/ultra/v1):
//   GET /order → quote plus unsigned transaction for live execution
// =============================================================================

pub mod filter;
pub mod helius_ws;
pub mod jupiter;
pub mod shredstream;
pub mod ultra;
pub mod yellowstone;

pub use filter::{EbpfFilter, FilterRule};
pub use helius_ws::{AlchemyTransactionStream, HeliusTransactionStream, DEX_PROGRAMS};
pub use jupiter::{build_token_info_map, JupiterMonitor, KnownToken, TokenInfo, TOKENS};
pub use shredstream::ShredEvent;
pub use ultra::{build_ultra_client, request_ultra_order, UltraOrder, JUPITER_ULTRA_BASE};
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
