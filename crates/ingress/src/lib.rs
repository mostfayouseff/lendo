// =============================================================================
// INGRESS CRATE — Live data ingress from Helius, Alchemy, and Jupiter
//
// PRIMARY stream:   Helius WebSocket (transactionSubscribe + logsSubscribe)
// FALLBACK stream:  Alchemy WebSocket (logsSubscribe)
// PRICE feed:       Multi-source Jupiter price monitor (self-healing)
// SWAP EXECUTION:   Jupiter /v6/swap-instructions → real v0 VersionedTransaction
// =============================================================================

pub mod filter;
pub mod helius_ws;
pub mod jupiter;
pub mod shredstream;
pub mod swap_builder;
pub mod ultra;
pub mod yellowstone;

pub use filter::{EbpfFilter, FilterRule};
pub use helius_ws::{AlchemyTransactionStream, HeliusTransactionStream, DEX_PROGRAMS};
pub use jupiter::{build_token_info_map, JupiterMonitor, KnownToken, TokenInfo, TOKENS};
pub use shredstream::{MockShredStream, ShredEvent};
pub use swap_builder::{
    build_signed_swap_transaction, build_swap_client, BuiltSwapTransaction,
    JupiterAccountMeta, JupiterInstruction, QuoteResponse, SwapInstructionsResponse,
};
pub use ultra::{
    build_ultra_client, get_best_route_and_transaction, RouteExecutionData, RoutePlanStep,
    SwapInfo, TokenMetadata, UltraOrderResponse,
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
