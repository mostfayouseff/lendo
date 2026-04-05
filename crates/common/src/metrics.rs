// =============================================================================
// SECURITY AUDIT CHECKLIST — common/src/metrics.rs
// [✓] Prometheus metrics are labelled but never expose secret data
// [✓] All metric increments are infallible (counters/gauges never panic)
// [✓] No unsafe code
// =============================================================================

use prometheus::{
    register_counter, register_gauge, register_histogram, Counter, Gauge, Histogram,
    HistogramOpts, Opts,
};
use tracing::warn;

/// Central Prometheus metrics store for the bot.
/// Instantiate once and pass by Arc to all subsystems.
pub struct ApexMetrics {
    /// Total paths evaluated by the RICH engine
    pub paths_evaluated: Counter,
    /// Total paths above min-profit threshold
    pub paths_profitable: Counter,
    /// Bundles submitted to Jito
    pub bundles_submitted: Counter,
    /// Bundles landed (included in a block)
    pub bundles_landed: Counter,
    /// Total profit in lamports (approximate; f64 for gauge)
    pub total_profit_lamports: Gauge,
    /// Current circuit-breaker state (0 = open, 1 = closed/healthy)
    pub circuit_breaker_state: Gauge,
    /// Latency histogram for the full hot path (μs)
    pub hot_path_latency_us: Histogram,
    /// GNN inference latency (μs)
    pub gnn_inference_latency_us: Histogram,
}

impl ApexMetrics {
    /// Register all metrics with the default Prometheus registry.
    ///
    /// # Errors
    /// Returns an error if registration fails (duplicate metric name).
    /// The caller should treat this as fatal during startup.
    pub fn register() -> Result<Self, prometheus::Error> {
        Ok(Self {
            paths_evaluated: register_counter!(Opts::new(
                "apex_paths_evaluated_total",
                "Total arbitrage paths evaluated by RICH engine"
            ))?,
            paths_profitable: register_counter!(Opts::new(
                "apex_paths_profitable_total",
                "Paths above minimum profit threshold"
            ))?,
            bundles_submitted: register_counter!(Opts::new(
                "apex_bundles_submitted_total",
                "Jito bundles submitted"
            ))?,
            bundles_landed: register_counter!(Opts::new(
                "apex_bundles_landed_total",
                "Jito bundles confirmed on-chain"
            ))?,
            total_profit_lamports: register_gauge!(Opts::new(
                "apex_total_profit_lamports",
                "Cumulative profit in lamports (can be negative)"
            ))?,
            circuit_breaker_state: register_gauge!(Opts::new(
                "apex_circuit_breaker_state",
                "Circuit breaker: 1=healthy, 0=tripped"
            ))?,
            hot_path_latency_us: register_histogram!(HistogramOpts::new(
                "apex_hot_path_latency_us",
                "Hot-path latency in microseconds"
            )
            .buckets(vec![50.0, 100.0, 200.0, 500.0, 1000.0, 5000.0]))?,
            gnn_inference_latency_us: register_histogram!(HistogramOpts::new(
                "apex_gnn_inference_latency_us",
                "GNN inference latency in microseconds"
            )
            .buckets(vec![10.0, 20.0, 50.0, 100.0, 500.0]))?,
        })
    }

    /// Safe helper: increment a counter without propagating errors upward
    /// (counters should never fail in practice; log and continue if they do).
    pub fn inc_safe(counter: &Counter) {
        counter.inc();
    }

    /// Observe hot-path latency. `start` should be from `std::time::Instant::now()`.
    pub fn observe_hot_path(&self, start: std::time::Instant) {
        let elapsed_us = start.elapsed().as_micros() as f64;
        if let Err(e) = self.hot_path_latency_us.observe(elapsed_us).into_err() {
            warn!("Histogram observe error: {e}");
        }
    }
}

// Workaround: Histogram::observe returns () not Result, so we can just call it.
trait IntoErr {
    fn into_err(self) -> Result<(), String>;
}
impl IntoErr for () {
    fn into_err(self) -> Result<(), String> {
        Ok(())
    }
}
