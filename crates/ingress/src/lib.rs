// crates/ingress/src/lib.rs
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
};
pub use ultra::{
    build_ultra_client,
    get_best_route_and_transaction,
    RouteExecutionData,
    RoutePlanStep,
    SwapInfo,
    UltraOrderResponse,
};
pub use yellowstone::{MockYellowstoneStream, SlotUpdate};

// Jupiter price monitor exports
pub use jupiter::{
    JupiterMonitor,
    build_token_info_map,
    KnownToken,
    TokenInfo,
    TOKENS,
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
