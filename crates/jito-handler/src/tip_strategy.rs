// =============================================================================
// Adaptive Tip Strategy
//
// Replaces the static TipCalculator with a dynamic strategy that learns from
// bundle outcomes and adjusts competitiveness accordingly.
//
// Starting tip fraction: 30% of expected profit.
// On rejection / 429  → increase fraction (more competitive).
// On consistent wins  → decrease fraction (preserve more profit).
//
// Hard limits:
//   min: 15% of profit (floor), but never below MIN_TIP_LAMPORTS absolute.
//   max: 70% of profit (never tip away all profit).
// =============================================================================

use tracing::{debug, info};

const INITIAL_TIP_FRACTION: f64 = 0.30;
const MIN_TIP_FRACTION: f64 = 0.15;
const MAX_TIP_FRACTION: f64 = 0.70;
const MIN_TIP_LAMPORTS: u64 = 10_000;

/// Feedback from a bundle submission for tip learning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipOutcome {
    /// Block engine accepted the bundle.
    Accepted,
    /// Block engine or validator rejected the bundle.
    Rejected,
    /// HTTP 429 rate limit — treat as a signal to be more competitive.
    RateLimit,
}

/// Adaptive tip strategy that adjusts the tip fraction based on submission outcomes.
pub struct TipStrategy {
    tip_fraction: f64,
    total_accepted: u64,
    total_rejected: u64,
    total_tip_lamports_spent: u64,
    total_tip_lamports_accepted: u64,
}

impl Default for TipStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl TipStrategy {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tip_fraction: INITIAL_TIP_FRACTION,
            total_accepted: 0,
            total_rejected: 0,
            total_tip_lamports_spent: 0,
            total_tip_lamports_accepted: 0,
        }
    }

    /// Compute the tip in lamports for a given expected profit.
    ///
    /// Returns at least `MIN_TIP_LAMPORTS` and at most `MAX_TIP_FRACTION * profit`.
    #[must_use]
    pub fn compute_tip(&self, expected_profit_lamports: u64) -> u64 {
        if expected_profit_lamports == 0 {
            return MIN_TIP_LAMPORTS;
        }
        let raw = (expected_profit_lamports as f64 * self.tip_fraction) as u64;
        let ceiling = (expected_profit_lamports as f64 * MAX_TIP_FRACTION) as u64;
        raw.clamp(MIN_TIP_LAMPORTS, ceiling.max(MIN_TIP_LAMPORTS))
    }

    /// Record the outcome of a submission and update the tip fraction.
    pub fn record_outcome(&mut self, outcome: TipOutcome, tip_paid: u64) {
        self.total_tip_lamports_spent =
            self.total_tip_lamports_spent.saturating_add(tip_paid);

        match outcome {
            TipOutcome::Accepted => {
                self.tip_fraction = (self.tip_fraction * 0.95).max(MIN_TIP_FRACTION);
                self.total_accepted += 1;
                self.total_tip_lamports_accepted =
                    self.total_tip_lamports_accepted.saturating_add(tip_paid);
                info!(
                    tip_fraction_pct = format!("{:.1}%", self.tip_fraction * 100.0),
                    total_accepted   = self.total_accepted,
                    efficiency       = format!("{:.3}", self.tip_efficiency()),
                    "TipStrategy: accepted — tip fraction decreased"
                );
            }
            TipOutcome::Rejected | TipOutcome::RateLimit => {
                self.tip_fraction = (self.tip_fraction * 1.15).min(MAX_TIP_FRACTION);
                self.total_rejected += 1;
                debug!(
                    tip_fraction_pct = format!("{:.1}%", self.tip_fraction * 100.0),
                    outcome          = ?outcome,
                    "TipStrategy: not accepted — tip fraction increased"
                );
            }
        }
    }

    /// Tip efficiency: accepted bundles per million lamports spent on tips.
    /// Higher value = better ROI on tips.
    #[must_use]
    pub fn tip_efficiency(&self) -> f64 {
        if self.total_tip_lamports_spent == 0 {
            return 0.0;
        }
        self.total_accepted as f64 / (self.total_tip_lamports_spent as f64 / 1_000_000.0)
    }

    /// Acceptance rate across all recorded outcomes.
    #[must_use]
    pub fn acceptance_rate(&self) -> f64 {
        let total = self.total_accepted + self.total_rejected;
        if total == 0 {
            return 1.0;
        }
        self.total_accepted as f64 / total as f64
    }

    /// Current tip fraction as a human-readable percentage.
    #[must_use]
    pub fn tip_fraction_pct(&self) -> f64 {
        self.tip_fraction * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tip_within_bounds() {
        let strat = TipStrategy::new();
        let tip = strat.compute_tip(100_000);
        assert!(tip >= MIN_TIP_LAMPORTS);
        assert!(tip <= (100_000f64 * MAX_TIP_FRACTION) as u64);
    }

    #[test]
    fn zero_profit_returns_min() {
        let strat = TipStrategy::new();
        assert_eq!(strat.compute_tip(0), MIN_TIP_LAMPORTS);
    }

    #[test]
    fn accepted_decreases_fraction() {
        let mut strat = TipStrategy::new();
        let before = strat.tip_fraction;
        strat.record_outcome(TipOutcome::Accepted, 10_000);
        assert!(strat.tip_fraction <= before);
    }

    #[test]
    fn rejected_increases_fraction() {
        let mut strat = TipStrategy::new();
        let before = strat.tip_fraction;
        strat.record_outcome(TipOutcome::Rejected, 10_000);
        assert!(strat.tip_fraction >= before);
    }

    #[test]
    fn fraction_bounded() {
        let mut strat = TipStrategy::new();
        for _ in 0..100 {
            strat.record_outcome(TipOutcome::Rejected, 10_000);
        }
        assert!(strat.tip_fraction <= MAX_TIP_FRACTION + f64::EPSILON);
        let mut strat2 = TipStrategy::new();
        for _ in 0..100 {
            strat2.record_outcome(TipOutcome::Accepted, 10_000);
        }
        assert!(strat2.tip_fraction >= MIN_TIP_FRACTION - f64::EPSILON);
    }
}
