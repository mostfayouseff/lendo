// =============================================================================
// SECURITY AUDIT CHECKLIST — safety/src/lib.rs
// [✓] Atomic revert: all state changes are only committed on Success
// [✓] Pre-simulation always runs before real submission
// [✓] No panics — all safety checks return Result
// [✓] No unsafe code
// =============================================================================

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod atomic_revert;
pub mod pre_simulation;

pub use atomic_revert::AtomicRevertGuard;
pub use pre_simulation::{PreSimulator, SimulationResult};
