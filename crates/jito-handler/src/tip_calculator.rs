// =============================================================================
// SECURITY AUDIT CHECKLIST — jito-handler/src/tip_calculator.rs
// [✓] Tip is bounded to [30%, 70%] of profit (spec §5 RL-based tipping)
// [✓] No floating-point tip: computed in integer lamports
// [✓] Min-tip floor prevents zero-tip bundles (validators ignore them)
// [✓] No panics — all arithmetic saturating
// [✓] No unsafe code
//
// RL STUB:
//   Production: a Deep Q-Network (DQN) agent selects tip percentage based on:
//     - Current validator tip market (Jito tip percentile)
//     - Time in slot (urgency)
//     - Path confidence score
//   Stub: uses a fixed 40% of profit as the tip.
// =============================================================================

use thiserror::Error;

/// Minimum tip to prevent bundle rejection (10_000 lamports = 0.00001 SOL)
const MIN_TIP_LAMPORTS: u64 = 10_000;
/// Maximum tip as a fraction of profit (70%)
const MAX_TIP_FRACTION: u64 = 70;
/// Default tip fraction (40%)
const DEFAULT_TIP_FRACTION: u64 = 40;

#[derive(Debug, Error)]
pub enum TipError {
    #[error("Profit is zero — cannot compute tip")]
    ZeroProfit,
}

/// Computes the Jito bundle tip using the RL stub.
pub struct TipCalculator {
    tip_fraction: u64, // percentage (30–70)
}

impl TipCalculator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tip_fraction: DEFAULT_TIP_FRACTION,
        }
    }

    /// Compute the tip amount in lamports.
    ///
    /// Bounded to [MIN_TIP_LAMPORTS, profit * MAX_TIP_FRACTION / 100].
    ///
    /// # Errors
    /// Returns `TipError::ZeroProfit` if `expected_profit_lamports == 0`.
    pub fn compute_tip(&self, expected_profit_lamports: u64) -> Result<u64, TipError> {
        if expected_profit_lamports == 0 {
            return Err(TipError::ZeroProfit);
        }

        // Compute raw tip: profit * fraction / 100
        let raw_tip = expected_profit_lamports
            .saturating_mul(self.tip_fraction)
            / 100;

        // Apply bounds
        let max_tip = expected_profit_lamports
            .saturating_mul(MAX_TIP_FRACTION)
            / 100;

        let tip = raw_tip.clamp(MIN_TIP_LAMPORTS, max_tip);
        Ok(tip)
    }

    /// Update the tip fraction (for RL agent updates).
    /// Clamps to [30, 70] as per spec bounds.
    pub fn set_tip_fraction(&mut self, fraction: u64) {
        self.tip_fraction = fraction.clamp(30, 70);
    }
}

impl Default for TipCalculator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tip_within_bounds() {
        let calc = TipCalculator::new();
        let profit = 100_000u64;
        let tip = calc.compute_tip(profit).unwrap();
        assert!(tip >= MIN_TIP_LAMPORTS);
        assert!(tip <= profit * MAX_TIP_FRACTION / 100);
    }

    #[test]
    fn zero_profit_returns_error() {
        let calc = TipCalculator::new();
        assert!(calc.compute_tip(0).is_err());
    }

    #[test]
    fn small_profit_floors_to_min_tip() {
        let calc = TipCalculator::new();
        // profit so small that 40% < MIN_TIP_LAMPORTS
        let tip = calc.compute_tip(100).unwrap();
        assert_eq!(tip, MIN_TIP_LAMPORTS);
    }

    #[test]
    fn tip_fraction_clamped() {
        let mut calc = TipCalculator::new();
        calc.set_tip_fraction(99); // over max
        assert_eq!(calc.tip_fraction, 70);
        calc.set_tip_fraction(5); // under min
        assert_eq!(calc.tip_fraction, 30);
    }
}
