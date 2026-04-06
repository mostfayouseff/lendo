// =============================================================================
// Adaptive Cooldown Engine
//
// Replaces static cooldown with a dynamic system that tracks submission
// outcomes and adjusts the inter-submission delay accordingly.
//
// Behavior:
//   Success   → cooldown *= 0.8  (more aggressive)
//   RateLimit → cooldown *= 1.5  (significant back-off)
//   Failure   → cooldown *= 1.1  (minor back-off)
//
// Jitter: +/- 10% randomisation to prevent thundering-herd synchronisation.
//
// Dead-market detection:
//   10+ consecutive 429s → pause execution for 5–15 seconds.
//   On resumption the cooldown is at MAX to allow organic recovery.
// =============================================================================

use std::time::{Duration, Instant};
use tracing::{info, warn};

const MIN_COOLDOWN_MS: f64 = 300.0;
const MAX_COOLDOWN_MS: f64 = 5_000.0;
const INITIAL_COOLDOWN_MS: f64 = 1_200.0;
const JITTER_FRACTION: f64 = 0.10;
const DEAD_MARKET_429_THRESHOLD: u32 = 10;
const DEAD_MARKET_BASE_PAUSE_SECS: u64 = 5;
const DEAD_MARKET_MAX_PAUSE_SECS: u64 = 15;

/// Outcome of a Jito bundle submission attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// Block engine accepted the bundle (may or may not land on-chain).
    Success,
    /// HTTP 429 — rate limited by Jito.
    RateLimit,
    /// Any other failure (signing, network error, bundle rejected).
    Failure,
}

/// Dynamic cooldown controller for Jito bundle submission pacing.
pub struct AdaptiveCooldown {
    current_ms: f64,
    consecutive_429s: u32,
    last_submit: Option<Instant>,
    last_success: Option<Instant>,
    dead_market_until: Option<Instant>,
    total_submissions: u64,
    total_429s: u64,
    total_successes: u64,
    total_failures: u64,
}

impl Default for AdaptiveCooldown {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveCooldown {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_ms: INITIAL_COOLDOWN_MS,
            consecutive_429s: 0,
            last_submit: None,
            last_success: None,
            dead_market_until: None,
            total_submissions: 0,
            total_429s: 0,
            total_successes: 0,
            total_failures: 0,
        }
    }

    /// Record the outcome of a submission and adjust the internal cooldown.
    pub fn record_outcome(&mut self, outcome: SubmitOutcome) {
        self.total_submissions += 1;
        self.last_submit = Some(Instant::now());

        match outcome {
            SubmitOutcome::Success => {
                self.current_ms = (self.current_ms * 0.8).max(MIN_COOLDOWN_MS);
                self.consecutive_429s = 0;
                self.last_success = Some(Instant::now());
                self.dead_market_until = None;
                self.total_successes += 1;
                info!(
                    cooldown_ms      = self.current_ms as u64,
                    total_successes  = self.total_successes,
                    success_rate_pct = format!("{:.1}%", self.success_rate() * 100.0),
                    "AdaptiveCooldown: success — cooldown decreased"
                );
            }

            SubmitOutcome::RateLimit => {
                self.current_ms = (self.current_ms * 1.5).min(MAX_COOLDOWN_MS);
                self.consecutive_429s += 1;
                self.total_429s += 1;

                if self.consecutive_429s >= DEAD_MARKET_429_THRESHOLD {
                    let extra = u64::from(
                        (self.consecutive_429s - DEAD_MARKET_429_THRESHOLD).min(10),
                    );
                    let pause_secs = (DEAD_MARKET_BASE_PAUSE_SECS + extra)
                        .min(DEAD_MARKET_MAX_PAUSE_SECS);
                    warn!(
                        consecutive_429s = self.consecutive_429s,
                        pause_secs,
                        "AdaptiveCooldown: DEAD MARKET — entering execution pause"
                    );
                    self.dead_market_until =
                        Some(Instant::now() + Duration::from_secs(pause_secs));
                    self.consecutive_429s = 0;
                } else {
                    warn!(
                        cooldown_ms      = self.current_ms as u64,
                        consecutive_429s = self.consecutive_429s,
                        total_429s       = self.total_429s,
                        "AdaptiveCooldown: 429 — cooldown increased"
                    );
                }
            }

            SubmitOutcome::Failure => {
                self.current_ms = (self.current_ms * 1.1).min(MAX_COOLDOWN_MS);
                self.consecutive_429s = 0;
                self.total_failures += 1;
            }
        }
    }

    /// Whether enough time has passed since the last submission and we are
    /// not in a dead-market pause.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        if let Some(until) = self.dead_market_until {
            if Instant::now() < until {
                return false;
            }
        }
        match self.last_submit {
            None => true,
            Some(last) => last.elapsed() >= self.cooldown_with_jitter(),
        }
    }

    /// Current base cooldown in milliseconds (without jitter).
    #[must_use]
    pub fn current_ms(&self) -> u64 {
        self.current_ms as u64
    }

    /// Whether we are currently in a dead-market pause.
    #[must_use]
    pub fn is_dead_market(&self) -> bool {
        self.dead_market_until
            .map(|u| Instant::now() < u)
            .unwrap_or(false)
    }

    /// Seconds since the last successful submission, or None if never.
    #[must_use]
    pub fn secs_since_success(&self) -> Option<u64> {
        self.last_success.map(|t| t.elapsed().as_secs())
    }

    /// Overall success rate (0.0 – 1.0).
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.total_submissions == 0 {
            return 1.0;
        }
        self.total_successes as f64 / self.total_submissions as f64
    }

    /// Overall 429 rate (0.0 – 1.0).
    #[must_use]
    pub fn rate_limit_rate(&self) -> f64 {
        if self.total_submissions == 0 {
            return 0.0;
        }
        self.total_429s as f64 / self.total_submissions as f64
    }

    /// Total submission counts for observability logging.
    #[must_use]
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (
            self.total_submissions,
            self.total_successes,
            self.total_429s,
            self.total_failures,
        )
    }

    fn cooldown_with_jitter(&self) -> Duration {
        let jitter_range = self.current_ms * JITTER_FRACTION;
        let jitter = (rand::random::<f64>() * 2.0 - 1.0) * jitter_range;
        let ms = (self.current_ms + jitter).clamp(MIN_COOLDOWN_MS, MAX_COOLDOWN_MS) as u64;
        Duration::from_millis(ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_ready() {
        let c = AdaptiveCooldown::new();
        assert!(c.is_ready());
    }

    #[test]
    fn success_decreases_cooldown() {
        let mut c = AdaptiveCooldown::new();
        let initial = c.current_ms;
        c.record_outcome(SubmitOutcome::Success);
        assert!(c.current_ms <= initial);
        assert!(c.current_ms >= MIN_COOLDOWN_MS);
    }

    #[test]
    fn rate_limit_increases_cooldown() {
        let mut c = AdaptiveCooldown::new();
        let initial = c.current_ms;
        c.record_outcome(SubmitOutcome::RateLimit);
        assert!(c.current_ms >= initial);
        assert!(c.current_ms <= MAX_COOLDOWN_MS);
    }

    #[test]
    fn dead_market_after_ten_429s() {
        let mut c = AdaptiveCooldown::new();
        for _ in 0..DEAD_MARKET_429_THRESHOLD {
            c.record_outcome(SubmitOutcome::RateLimit);
        }
        assert!(c.is_dead_market());
        assert!(!c.is_ready());
    }

    #[test]
    fn success_clears_dead_market() {
        let mut c = AdaptiveCooldown::new();
        for _ in 0..DEAD_MARKET_429_THRESHOLD {
            c.record_outcome(SubmitOutcome::RateLimit);
        }
        c.dead_market_until = Some(Instant::now() - std::time::Duration::from_secs(1));
        c.record_outcome(SubmitOutcome::Success);
        assert!(!c.is_dead_market());
    }
}
