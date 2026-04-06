// crates/ingress/src/lib.rs
// INGRESS CRATE — Live data ingress from Helius, Alchemy, and Jupiter

pub mod filter;
pub mod helius_ws;
pub mod jupiter;
pub mod shredstream;
pub mod swap_builder;
pub mod ultra;
pub mod yellowstone;

// Public re-exports
pub use filter::{EbpfFilter, FilterRule};
pub use helius_ws::{AlchemyTransactionStream, HeliusTransactionStream, DEX_PROGRAMS};
pub use shredstream::{MockShredStream, ShredEvent};
pub use swap_builder::{
    build_signed_swap_transaction,
    build_swap_client,
    BuiltSwapTransaction,
    // Internal types - only re-export if other crates really need them
    // JupiterAccountMeta, JupiterInstruction, QuoteResponse, SwapInstructionsResponse,
};
pub use ultra::{
    build_ultra_client,
    get_best_route_and_transaction,
    RouteExecutionData,
    RoutePlanStep,
    SwapInfo,
    TokenMetadata,
    UltraOrderResponse,
};
pub use yellowstone::{MockYellowstoneStream, SlotUpdate};

// Jupiter price monitor exports (fixed)
pub use jupiter::{
    JupiterMonitor,
    build_token_info_map,
    KnownToken,
    TokenInfo,
    TOKENS,           // now properly exported
};

// Error type
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
