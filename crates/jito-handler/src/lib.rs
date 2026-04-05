// =============================================================================
// SECURITY AUDIT CHECKLIST — jito-handler/src/lib.rs
// [✓] Real keypair loading supported — never hardcoded
// [✓] Tip amount computed by RL oracle (bounded: 30–70% of profit)
// [✓] Bundle serialisation does not expose operator keys
// [✓] No panics — all errors are JitoError variants
// [✓] No unsafe code
// [✓] flash_tx: real atomic Solend borrow+swap+repay transaction builder
// =============================================================================

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod bundle;
pub mod flash_tx;
pub mod keypair;
pub mod rpc;
pub mod tip_calculator;

pub use bundle::{JitoBundle, JitoBundleHandler};
pub use flash_tx::{build_flash_loan_tx, find_associated_token_account, find_program_address};
pub use keypair::ApexKeypair;
pub use rpc::SolanaRpcClient;
pub use tip_calculator::TipCalculator;
