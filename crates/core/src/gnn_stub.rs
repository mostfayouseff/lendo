// =============================================================================
// SECURITY AUDIT CHECKLIST — core/src/gnn_stub.rs
// [✓] GNN is a stub — no real model weights loaded (would require auth in prod)
// [✓] Inference is deterministic given the same input (no hidden state)
// [✓] Output confidence is clamped to [0.0, 1.0] — no NaN/Inf propagation
// [✓] No unsafe code
// [✓] No panics — all arithmetic on f32 with explicit NaN guards
//
// ARCHITECTURE NOTE:
//   Production implementation would use candle-core (pure Rust tensor lib)
//   or a tch-rs (libtorch) binding to run a pre-trained GNN that predicts
//   path profitability from order-book graph features.
//   Stub replaces the model with a heuristic for prototype demonstration.
//
// PERFORMANCE (simulated):
//   Stub inference: ~50ns per path
//   Production GNN (4-layer GCN, 64 hidden): target ~15μs on CPU
//   With ONNX Runtime: target ~5μs
// =============================================================================

use common::types::ArbPath;
use tracing::trace;

/// GNN oracle: predicts profitability confidence for an arbitrage path.
pub struct GnnOracle {
    /// Feature weights (stub: hand-tuned heuristics)
    liquidity_weight: f32,
    hop_penalty: f32,
    min_profit_weight: f32,
}

impl GnnOracle {
    /// Construct with default heuristic weights.
    #[must_use]
    pub fn new() -> Self {
        Self {
            liquidity_weight: 0.4,
            hop_penalty: 0.1,
            min_profit_weight: 0.5,
        }
    }

    /// Run inference on a candidate arbitrage path.
    /// Returns a confidence score in [0.0, 1.0].
    ///
    /// Heuristic (stub):
    ///   score = liquidity_feature * w1 - hops * w2 + profit_feature * w3
    ///
    /// Where:
    ///   liquidity_feature = min(total_liquidity_lamports / 10^10, 1.0)
    ///   profit_feature    = min(expected_profit / min_viable_profit, 1.0)
    #[must_use]
    pub fn infer(&self, path: &ArbPath) -> f32 {
        let hops = path.edges.len() as f32;

        // Liquidity feature: normalised total liquidity across all edges
        let total_liq: u64 = path
            .edges
            .iter()
            .map(|e| e.liquidity_lamports)
            .fold(0u64, |acc, l| acc.saturating_add(l));
        let liq_feature = (total_liq as f32 / 1e10_f32).min(1.0_f32).max(0.0_f32);

        // Profit feature: normalised against a 10_000 lamport minimum
        let profit_feature = (path.expected_profit_lamports as f32 / 10_000.0_f32)
            .min(1.0_f32)
            .max(0.0_f32);

        // Raw score
        let raw = self.liquidity_weight * liq_feature
            - self.hop_penalty * hops
            + self.min_profit_weight * profit_feature;

        // Sigmoid activation: σ(x) = 1 / (1 + e^{-x})
        // NaN guard: if raw is NaN (shouldn't happen), return 0.0
        let score = sigmoid(raw);
        trace!(?path.expected_profit_lamports, liq_feature, profit_feature, score, "GNN inference");
        score
    }
}

impl Default for GnnOracle {
    fn default() -> Self {
        Self::new()
    }
}

/// Numerically stable sigmoid. Returns 0.0 for NaN input.
#[inline]
fn sigmoid(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() {
        return if x > 0.0 { 1.0 } else { 0.0 };
    }
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{Dex, MarketEdge, RichColor, TokenMint};
    use rust_decimal::Decimal;

    fn dummy_path(profit: u64, hops: usize, liquidity: u64) -> ArbPath {
        let tok = TokenMint::new([0u8; 32]);
        ArbPath {
            edges: (0..hops)
                .map(|_| MarketEdge {
                    from: tok.clone(),
                    to: tok.clone(),
                    dex: Dex::Raydium,
                    log_weight: Decimal::new(-5, 2),
                    liquidity_lamports: liquidity,
                    slot: 1,
                })
                .collect(),
            expected_profit_lamports: profit,
            gnn_confidence: 0.0,
            rich_color: RichColor::Gray,
        }
    }

    #[test]
    fn confidence_in_range() {
        let oracle = GnnOracle::new();
        for profit in [0, 1000, 50000, 1_000_000] {
            let path = dummy_path(profit, 3, 1_000_000_000);
            let score = oracle.infer(&path);
            assert!((0.0..=1.0).contains(&score), "score {score} out of range");
        }
    }

    #[test]
    fn higher_profit_yields_higher_confidence() {
        let oracle = GnnOracle::new();
        let low = oracle.infer(&dummy_path(1_000, 3, 1_000_000_000));
        let high = oracle.infer(&dummy_path(1_000_000, 3, 1_000_000_000));
        assert!(high > low, "Higher profit should yield higher confidence");
    }

    #[test]
    fn sigmoid_bounds() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(100.0) > 0.99);
        assert!(sigmoid(-100.0) < 0.01);
        assert_eq!(sigmoid(f32::NAN), 0.0);
        assert_eq!(sigmoid(f32::INFINITY), 1.0);
        assert_eq!(sigmoid(f32::NEG_INFINITY), 0.0);
    }
}
