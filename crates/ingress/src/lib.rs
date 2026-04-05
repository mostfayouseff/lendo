// =============================================================================
// SECURITY AUDIT CHECKLIST — ingress/src/lib.rs
// [✓] Zero-copy parsing: pointer offsets only, no allocations in hot path
// [✓] All slice accesses bounds-checked (get() not index[])
// [✓] No panics — all errors propagated as IngressError
// [✓] eBPF-style filter rejects oversized/malformed packets before processing
// [✓] No unsafe code — safe Rust achieves zero-copy via bytes::Bytes
// [✓] Jupiter monitor uses TLS HTTP and public endpoints — no API key required
// [✓] Alchemy stream uses TLS WSS — API key transmitted only in encrypted channel
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
