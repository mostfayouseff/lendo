// =============================================================================
// SECURITY AUDIT CHECKLIST — risk-oracle/src/lib.rs
// [✓] Circuit breaker is the last line of defense — checked atomically
// [✓] No panics — all state transitions return Result
// [✓] No unsafe code
// [✓] Self-optimizer adjusts parameters within hard safety bounds
// =============================================================================

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod anomaly;
pub mod circuit_breaker;
pub mod deep_q;
pub mod self_optimizer;

pub use anomaly::AnomalyDetector;
pub use circuit_breaker::{CircuitBreaker, CircuitState};
pub use deep_q::{DeepQOracle, DqnState};
pub use self_optimizer::{SelfOptimizer, TradingParams};
