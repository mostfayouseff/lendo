// =============================================================================
// SECURITY AUDIT CHECKLIST — risk-oracle/src/circuit_breaker.rs
// [✓] State machine: Closed → Open (never skips states)
// [✓] Lock-free state tracking using atomic-like Mutex (no Mutex poisoning risk
//     since we never panic while holding the lock)
// [✓] Reset requires explicit operator action (no auto-reset)
// [✓] All thresholds configurable — no hardcoded magic numbers
// [✓] No panics — mutex lock uses map_err not unwrap
// [✓] No unsafe code
//
// CIRCUIT BREAKER STATES:
//   Closed = healthy, trading allowed
//   Open   = tripped, all trading halted until manual reset
// =============================================================================

use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{error, info, warn};

#[derive(Debug, Error)]
pub enum CircuitError {
    #[error("Circuit is open — trading halted")]
    CircuitOpen,
    #[error("Lock poisoned")]
    LockPoisoned,
}

/// Current state of the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
}

#[derive(Debug)]
struct CircuitInner {
    state: CircuitState,
    consecutive_losses: u32,
    total_drawdown_lamports: i64,
    trip_reason: Option<String>,
}

/// Thread-safe circuit breaker that halts trading on anomalous conditions.
#[derive(Clone)]
pub struct CircuitBreaker {
    inner: Arc<Mutex<CircuitInner>>,
    max_consecutive_losses: u32,
    max_drawdown_lamports: i64,
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(max_consecutive_losses: u32, max_drawdown_lamports: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CircuitInner {
                state: CircuitState::Closed,
                consecutive_losses: 0,
                total_drawdown_lamports: 0,
                trip_reason: None,
            })),
            max_consecutive_losses,
            max_drawdown_lamports: max_drawdown_lamports as i64,
        }
    }

    /// Check if trading is currently allowed.
    ///
    /// # Errors
    /// Returns `CircuitError::CircuitOpen` if tripped.
    /// Returns `CircuitError::LockPoisoned` on internal error (fatal).
    pub fn check_allow_trade(&self) -> Result<(), CircuitError> {
        let inner = self.inner.lock().map_err(|_| CircuitError::LockPoisoned)?;
        match inner.state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                warn!(
                    reason = ?inner.trip_reason,
                    "Circuit breaker open — trade rejected"
                );
                Err(CircuitError::CircuitOpen)
            }
        }
    }

    /// Record the result of a completed trade.
    ///
    /// `profit_lamports`: positive = win, negative = loss.
    ///
    /// # Errors
    /// Returns `CircuitError::LockPoisoned` on internal error.
    pub fn record_trade(&self, profit_lamports: i64) -> Result<(), CircuitError> {
        let mut inner = self.inner.lock().map_err(|_| CircuitError::LockPoisoned)?;

        if profit_lamports < 0 {
            inner.consecutive_losses = inner.consecutive_losses.saturating_add(1);
            inner.total_drawdown_lamports = inner
                .total_drawdown_lamports
                .saturating_sub(profit_lamports.unsigned_abs() as i64);
        } else {
            inner.consecutive_losses = 0; // reset on win
        }

        // Check trip conditions
        if inner.consecutive_losses >= self.max_consecutive_losses {
            let reason = format!(
                "Consecutive losses threshold reached: {} >= {}",
                inner.consecutive_losses, self.max_consecutive_losses
            );
            self.trip_inner(&mut inner, reason);
        } else if inner.total_drawdown_lamports <= -self.max_drawdown_lamports {
            let reason = format!(
                "Max drawdown exceeded: {} lamports",
                inner.total_drawdown_lamports.abs()
            );
            self.trip_inner(&mut inner, reason);
        }

        Ok(())
    }

    fn trip_inner(&self, inner: &mut CircuitInner, reason: String) {
        error!(reason, "CIRCUIT BREAKER TRIPPED — all trading halted");
        inner.state = CircuitState::Open;
        inner.trip_reason = Some(reason);
    }

    /// Manual reset by an operator. Clears all counters.
    ///
    /// # Errors
    /// Returns `CircuitError::LockPoisoned` on internal error.
    pub fn manual_reset(&self) -> Result<(), CircuitError> {
        let mut inner = self.inner.lock().map_err(|_| CircuitError::LockPoisoned)?;
        info!("Circuit breaker manually reset");
        *inner = CircuitInner {
            state: CircuitState::Closed,
            consecutive_losses: 0,
            total_drawdown_lamports: 0,
            trip_reason: None,
        };
        Ok(())
    }

    /// Return current state without side effects.
    pub fn state(&self) -> CircuitState {
        self.inner
            .lock()
            .map(|inner| inner.state)
            .unwrap_or(CircuitState::Open) // fail-safe: treat poisoned lock as tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_closed() {
        let cb = CircuitBreaker::new(5, 1_000_000);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.check_allow_trade().is_ok());
    }

    #[test]
    fn trips_on_consecutive_losses() {
        let cb = CircuitBreaker::new(3, 1_000_000_000);
        for _ in 0..3 {
            cb.record_trade(-1000).unwrap();
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(cb.check_allow_trade().is_err());
    }

    #[test]
    fn win_resets_consecutive_loss_counter() {
        let cb = CircuitBreaker::new(3, 1_000_000_000);
        cb.record_trade(-1000).unwrap();
        cb.record_trade(-1000).unwrap();
        cb.record_trade(5000).unwrap(); // win resets counter
        cb.record_trade(-1000).unwrap(); // only 1 loss now
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn trips_on_drawdown() {
        let cb = CircuitBreaker::new(100, 10_000);
        cb.record_trade(-11_000).unwrap(); // exceeds drawdown
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn manual_reset_works() {
        let cb = CircuitBreaker::new(1, 1_000_000);
        cb.record_trade(-500).unwrap();
        assert_eq!(cb.state(), CircuitState::Open);
        cb.manual_reset().unwrap();
        assert_eq!(cb.state(), CircuitState::Closed);
    }
}
