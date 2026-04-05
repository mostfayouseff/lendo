// =============================================================================
// SECURITY AUDIT CHECKLIST — solana-program/src/lib.rs
// [✓] No real on-chain deployment in prototype (simulation only)
// [✓] Zero-selector dispatch pattern (no function selector collisions)
// [✓] Per-swap balance checks — revert if Final_Balance < Initial + Min_Profit
// [✓] Address Lookup Table (ALT v0) transaction format stubs
// [✓] No panics — all errors propagated as ProgramError
// [✓] No unsafe code
// [✓] No hardcoded program IDs
// =============================================================================

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod instruction;

pub use instruction::{ApexInstruction, ProgramError};
