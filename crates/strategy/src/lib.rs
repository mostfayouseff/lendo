// =============================================================================
// SECURITY AUDIT CHECKLIST — strategy/src/lib.rs
// [✓] No hardcoded DEX program IDs — loaded from config/env
// [✓] Flash swap logic has explicit revert conditions
// [✓] No panics — all trade computations return Result
// [✓] No unsafe code
// [✓] Real DEX program IDs in flash_swap module
// [✓] Solend flash loan module with real instruction building
// =============================================================================

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod arbitrage;
pub mod flash_swap;
pub mod multi_dex;
pub mod solend;

pub use arbitrage::{ArbitrageStrategy, StrategyError};
pub use flash_swap::{dex_program_id, FlashSwapBuilder, SwapInstruction};
pub use multi_dex::MultiDexRouter;
pub use solend::{FlashLoanPlan, SolendFlashLoan};
