// =============================================================================
// SECURITY AUDIT CHECKLIST — jito-handler/src/lib.rs
// [✓] Real keypair loading supported — never hardcoded
// [✓] Tip amount computed by RL oracle (bounded: 30–70% of profit)
// [✓] Bundle serialisation does not expose operator keys
// [✓] No panics — all errors are JitoError variants
// [✓] No unsafe code
// [✓] flash_tx:    real atomic Solend borrow+swap+repay transaction builder
// [✓] transaction: signs Jupiter Ultra transactions and appends Jito tip instruction
// =============================================================================

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod adaptive_cooldown;
pub mod bundle;
pub mod flash_tx;
pub mod flash_tx_v2;
pub mod keypair;
pub mod rpc;
pub mod tip_calculator;
pub mod tip_strategy;
pub mod transaction;

pub use adaptive_cooldown::{AdaptiveCooldown, SubmitOutcome};
pub use bundle::{JitoBundle, JitoBundleHandler, select_random_tip_account};
pub use flash_tx::{find_associated_token_account, find_program_address};
pub use flash_tx_v2::{
    build_atomic_flash_v0, solend_repay_amount, AtomicAccountMeta, AtomicInstruction,
};
pub use keypair::ApexKeypair;
pub use rpc::SolanaRpcClient;
pub use tip_strategy::{TipOutcome, TipStrategy};
pub use transaction::{attach_jito_tip_and_sign, SignedTipTransaction};
