// =============================================================================
// INGRESS CRATE — Live data ingress from Helius, Alchemy, and Jupiter
//
// PRIMARY stream:  Helius WebSocket (transactionSubscribe + logsSubscribe)
// FALLBACK stream: Alchemy WebSocket (logsSubscribe)
// PRICE feed:      Multi-source Jupiter price monitor (self-healing)
// =============================================================================

pub mod filter;
pub mod helius_ws;
pub mod jupiter;
pub mod shredstream;
pub mod yellowstone;

pub use filter::{EbpfFilter, FilterRule};
pub use helius_ws::{AlchemyTransactionStream, HeliusTransactionStream, DEX_PROGRAMS};
pub use jupiter::{build_token_info_map, JupiterMonitor, KnownToken, TokenInfo, TOKENS};
pub use shredstream::{MockShredStream, ShredEvent};
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
