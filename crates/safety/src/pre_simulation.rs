// =============================================================================
// SECURITY AUDIT CHECKLIST — safety/src/pre_simulation.rs
// [✓] Simulation always runs before real submission (enforced by type system)
// [✓] SimulationResult is non-Copy — caller must explicitly handle it
// [✓] No network calls in simulation — runs on local fork state
// [✓] Profit guard re-validated in simulation (double-check before submission)
// [✓] No panics
// [✓] No unsafe code
//
// Phase 5: per-DEX fee rates (fee_bps per hop).
// Chain fix: exchange_rate per hop so the simulation correctly models
//   the actual price ratio at each DEX (e^(-log_weight)), not just fees.
//
// The hop tuple is now: (amount_in, min_out, fee_bps, exchange_rate)
//   amount_in     — initial capital for this hop (used for position sizing)
//   min_out       — per-hop minimum (0 = disabled; profit guard is the backstop)
//   fee_bps       — DEX-specific fee in basis points
//   exchange_rate — e^(-log_weight): price ratio for this leg
// =============================================================================

use solana_program_apex::instruction::{ApexInstruction, HopParam, MultiHopSwapParams};
use tracing::{debug, info, warn};

/// Result of pre-simulation.
#[derive(Debug)]
pub struct SimulationResult {
    pub success: bool,
    pub final_balance: u64,
    pub expected_profit_lamports: u64,
    pub error: Option<String>,
}

impl SimulationResult {
    /// Returns true if the simulation passed and profit meets the minimum.
    #[must_use]
    pub fn is_profitable(&self, min_profit: u64) -> bool {
        self.success && self.expected_profit_lamports >= min_profit
    }
}

/// Pre-simulation layer: validates arbitrage trades against a local fork
/// before real on-chain submission.
pub struct PreSimulator {
    /// Minimum profit to consider simulation successful (lamports).
    #[allow(dead_code)]
    min_profit_lamports: u64,
}

impl PreSimulator {
    #[must_use]
    pub fn new(min_profit_lamports: u64) -> Self {
        Self { min_profit_lamports }
    }

    /// Simulate a multi-hop chain swap.
    ///
    /// # Arguments
    /// * `initial_balance`     — capital deployed in lamports
    /// * `hops`                — (amount_in, min_out, fee_bps, exchange_rate) per hop
    ///                           fee_bps  = DEX-specific swap fee (Phase 5)
    ///                           exchange_rate = e^(-log_weight) for actual price movement
    /// * `min_profit_lamports` — minimum acceptable net profit
    pub fn simulate_swap(
        &self,
        initial_balance: u64,
        hops: &[(u64, u64, u16, f64)], // (amount_in, min_out, fee_bps, exchange_rate)
        min_profit_lamports: u64,
    ) -> SimulationResult {
        let hop_params: Vec<HopParam> = hops
            .iter()
            .map(|&(amount_in, min_amount_out, fee_bps, exchange_rate)| HopParam {
                amount_in,
                min_amount_out,
                pool_index: 0,
                fee_bps,
                exchange_rate,
            })
            .collect();

        let instruction = ApexInstruction::MultiHopSwap(MultiHopSwapParams {
            initial_balance,
            min_profit_lamports,
            hops: hop_params,
            lookup_table_indices: Vec::new(),
        });

        match instruction.execute_simulated(initial_balance) {
            Ok(final_balance) => {
                let profit = final_balance.saturating_sub(initial_balance);
                debug!(
                    initial_balance,
                    final_balance,
                    profit,
                    n_hops = hops.len(),
                    "Simulation succeeded"
                );
                info!(
                    initial_lamports = initial_balance,
                    final_lamports   = final_balance,
                    profit_lamports  = profit,
                    profit_sol       = format!("{:.6}", profit as f64 / 1e9),
                    n_hops           = hops.len(),
                    "PRE-SIM PASS"
                );
                SimulationResult {
                    success: true,
                    final_balance,
                    expected_profit_lamports: profit,
                    error: None,
                }
            }
            Err(e) => {
                warn!(error = %e, "Simulation failed");
                SimulationResult {
                    success: false,
                    final_balance: 0,
                    expected_profit_lamports: 0,
                    error: Some(e.to_string()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_simulation_neutral_rate() {
        // exchange_rate=1.0 (no arb), min_profit=0 → passes (fees only)
        let sim = PreSimulator::new(5_000);
        let result = sim.simulate_swap(
            1_000_000,
            &[(1_000_000, 0, 30, 1.0)],
            0,
        );
        assert!(result.success);
    }

    #[test]
    fn simulation_with_arb_exchange_rate_is_profitable() {
        // Favourable exchange rate (log_weight=-0.08 → exchange_rate≈1.0833)
        let sim = PreSimulator::new(5_000);
        let rate = (-(-0.08_f64)).exp(); // ≈1.0833
        let result = sim.simulate_swap(
            100_000_000,
            &[(100_000_000, 0, 30, rate)],
            5_000,
        );
        assert!(result.success, "Should pass with favourable exchange rate: {:?}", result.error);
        assert!(result.expected_profit_lamports >= 5_000);
    }

    #[test]
    fn three_hop_arb_passes_simulation() {
        let sim = PreSimulator::new(10_000);
        let r1 = (-(-0.08_f64)).exp();
        let r2 = (-(-0.05_f64)).exp();
        let r3 = (-(-0.06_f64)).exp();
        let result = sim.simulate_swap(
            100_000_000,
            &[
                (100_000_000, 0, 30, r1),
                (100_000_000, 0, 25, r2),
                (100_000_000, 0, 30, r3),
            ],
            10_000,
        );
        assert!(result.success, "3-hop arb should pass: {:?}", result.error);
        assert!(result.expected_profit_lamports > 10_000_000, "Should profit >10M lamports");
    }

    #[test]
    fn phoenix_fee_beats_orca_neutral_rate() {
        let sim = PreSimulator::new(0);
        let orca = sim.simulate_swap(1_000_000, &[(1_000_000, 0, 30, 1.0)], 0);
        let phoenix = sim.simulate_swap(1_000_000, &[(1_000_000, 0, 10, 1.0)], 0);
        assert!(
            phoenix.final_balance >= orca.final_balance,
            "Phoenix (10bps) should yield at least as much as Orca (30bps)"
        );
    }

    #[test]
    fn failed_simulation_on_impossible_profit() {
        let sim = PreSimulator::new(1_000_000_000);
        let result = sim.simulate_swap(1_000_000, &[(1_000_000, 0, 30, 1.0)], 1_000_000_000);
        assert!(!result.success);
    }

    #[test]
    fn is_profitable_check() {
        let result = SimulationResult {
            success: true,
            final_balance: 1_050_000,
            expected_profit_lamports: 50_000,
            error: None,
        };
        assert!(result.is_profitable(10_000));
        assert!(!result.is_profitable(100_000));
    }
}
