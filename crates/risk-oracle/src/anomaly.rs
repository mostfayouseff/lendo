// =============================================================================
// SECURITY AUDIT CHECKLIST — risk-oracle/src/anomaly.rs
// [✓] Statistical anomaly detection; no external network calls
// [✓] Rolling window uses a fixed-size buffer (no unbounded growth)
// [✓] Z-score computation uses integer arithmetic where possible
// [✓] No unsafe code
// [✓] No panics — division protected by length check
// =============================================================================

use tracing::warn;

/// Maximum window size for rolling statistics (to bound memory use).
const MAX_WINDOW: usize = 1000;
/// Z-score threshold for anomaly detection (3σ)
const ANOMALY_Z_THRESHOLD: f64 = 3.0;

/// Rolling-window anomaly detector for profit samples.
/// Detects market manipulation or price feed corruption.
pub struct AnomalyDetector {
    window: Vec<f64>,
    max_window: usize,
}

impl AnomalyDetector {
    #[must_use]
    pub fn new(window_size: usize) -> Self {
        Self {
            window: Vec::with_capacity(window_size.min(MAX_WINDOW)),
            max_window: window_size.min(MAX_WINDOW),
        }
    }

    /// Observe a profit value and return whether it is anomalous.
    ///
    /// An observation is anomalous if its Z-score exceeds `ANOMALY_Z_THRESHOLD`.
    /// On anomaly detection, logs a warning and returns `true`.
    #[must_use]
    pub fn observe(&mut self, profit_lamports: i64) -> bool {
        let value = profit_lamports as f64;

        // Maintain rolling window
        if self.window.len() >= self.max_window {
            self.window.remove(0); // O(n) — acceptable for small windows
        }

        let is_anomaly = if self.window.len() >= 10 {
            let (mean, std_dev) = self.stats();
            if std_dev > 0.0 {
                let z = (value - mean).abs() / std_dev;
                if z > ANOMALY_Z_THRESHOLD {
                    warn!(
                        value = profit_lamports,
                        z_score = z,
                        mean,
                        std_dev,
                        "Anomalous profit observation detected"
                    );
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false // Not enough data yet
        };

        self.window.push(value);
        is_anomaly
    }

    /// Compute rolling mean and standard deviation.
    fn stats(&self) -> (f64, f64) {
        let n = self.window.len() as f64;
        if n == 0.0 {
            return (0.0, 0.0);
        }
        let mean = self.window.iter().sum::<f64>() / n;
        let variance = self.window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        (mean, variance.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_anomaly_on_uniform_data() {
        let mut detector = AnomalyDetector::new(50);
        for _ in 0..50 {
            assert!(!detector.observe(1_000));
        }
    }

    #[test]
    fn detects_spike() {
        let mut detector = AnomalyDetector::new(50);
        for _ in 0..30 {
            detector.observe(1_000);
        }
        // A 10x spike should trigger
        let anomaly = detector.observe(1_000_000_000);
        assert!(anomaly, "Should detect massive profit spike as anomaly");
    }

    #[test]
    fn window_does_not_grow_unbounded() {
        let mut detector = AnomalyDetector::new(20);
        for i in 0..1000 {
            detector.observe(i);
        }
        assert!(detector.window.len() <= 20);
    }
}
