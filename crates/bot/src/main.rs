// =============================================================================
// APEX-MEV Neural Core 3.0 — LIVE TRADING MODE
//
// INGRESS PRIORITY:
//   1. Helius WebSocket (PRIMARY)   — wss://mainnet.helius-rpc.com/?api-key=<KEY>
//   2. Alchemy WebSocket (FALLBACK) — wss://solana-mainnet.g.alchemy.com/v2/<KEY>
//   Mock streams are disabled for production execution.
//
// PRICE DATA:
//   Jupiter Price API feeds the market matrix.
//   2. CoinGecko public API                                     (FALLBACK)
//   3. Stale cache                                              (LAST RESORT)
//
// EXECUTION — Jupiter Ultra API:
//   Base: https://api.jup.ag/ultra/v1/
//   GET /order returns a quote plus unsigned transaction.
//   The bot simulates that transaction, appends a configurable Jito tip
//   instruction, signs, sends via Solana RPC, and waits for confirmation.
//
// LOGGING:
//   "LIVE DATA SOURCE: HELIUS" — when Helius is active
//   "LIVE DATA SOURCE: ALCHEMY (FALLBACK)" — when Alchemy is active
//   "LIVE DATA SOURCE: JUPITER" — when Jupiter prices update
//   All wallet balances, trades, errors, and retries are logged
// =============================================================================

mod pnl;

use anyhow::{Context, Result};
use common::{ApexConfig, ApexMetrics};
use apex_core::MatrixBuilder;
use base64::Engine as _;
use ingress::{
    build_ultra_client, request_ultra_order,
    AlchemyTransactionStream, HeliusTransactionStream,
    JupiterMonitor,
};
use jito_handler::{
    attach_jito_tip_and_sign, ApexKeypair, JitoBundleHandler, SolanaRpcClient,
    AdaptiveCooldown, SubmitOutcome,
};
use pnl::{make_record, AtomicPnL, SessionStats};
use risk_oracle::{AnomalyDetector, CircuitBreaker, SelfOptimizer, TradingParams};
use safety::{AtomicRevertGuard, PreSimulator};
use solana_program_apex::instruction::dex_fee_bps;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use strategy::ArbitrageStrategy;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use ingress::ShredEvent;

#[tokio::main]
async fn main() -> Result<()> {
    // ── Logging ───────────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_env("RUST_LOG")
                .add_directive("apex_mev=info".parse().unwrap()),
        )
        .init();

    let startup_trace = std::time::Instant::now();
    info!(stage = "[START]", "Execution trace initialized");
    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║   APEX-MEV Neural Core 3.0  — LIVE MAINNET TRADING          ║");
    info!("╚══════════════════════════════════════════════════════════════╝");

    // ── Configuration ─────────────────────────────────────────────────────────
    let config = ApexConfig::from_env()
        .context("Failed to load configuration — check env vars")?;
    validate_required_environment(&config)?;
    info!(
        stage = "[CONFIG OK]",
        elapsed_ms = startup_trace.elapsed().as_millis(),
        simulation_only = config.simulation_only,
        rpc_url = %config.rpc_url,
        http_rpc_url = %config.http_rpc_url,
        keypair_path = %config.keypair_path,
        helius_configured = config.helius_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
        alchemy_configured = config.alchemy_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
        jupiter_configured = config.jupiter_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
        jito_tip_account = %config.jito_tip_account,
        jito_tip_lamports = config.jito_tip_lamports,
        "Configuration loaded and validated"
    );

    info!(
        simulation_only   = config.simulation_only,
        rpc_url           = %config.rpc_url,
        http_rpc_url      = %config.http_rpc_url,
        helius_active     = config.helius_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
        alchemy_active    = config.alchemy_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
        jupiter_key       = config.jupiter_api_key.is_some(),
        min_profit        = config.min_profit_lamports,
        max_hops          = config.max_hops,
        flash_loan        = config.flash_loan_enabled,
        "Configuration loaded"
    );

    // ── HTTP RPC connectivity check ────────────────────────────────────────────
    info!(url = %config.http_rpc_url, "Solana RPC: verifying connectivity (getSlot)");
    match SolanaRpcClient::new(&config.http_rpc_url) {
        Ok(rpc_check) => match rpc_check.get_slot().await {
            Ok(slot) => {
                info!(
                    stage = "[RPC OK]",
                    slot,
                    url = %config.http_rpc_url,
                    elapsed_ms = startup_trace.elapsed().as_millis(),
                    "Solana RPC getSlot OK"
                );
            }
            Err(e) => {
                anyhow::bail!("Solana RPC getSlot failed for {}: {e}", config.http_rpc_url);
            }
        },
        Err(e) => {
            anyhow::bail!("Solana RPC client init failed: {e}");
        }
    }

    // ── Shared current slot (AtomicU64) — written by background poller, read in hot loop ──
    let current_slot: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

    // ── Background slot poller ─────────────────────────────────────────────────
    {
        let rpc_url    = config.http_rpc_url.clone();
        let slot_share = current_slot.clone();
        tokio::spawn(async move {
            let rpc = match SolanaRpcClient::new(&rpc_url) {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Slot poller: could not build RPC client");
                    return;
                }
            };
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                match rpc.get_slot().await {
                    Ok(slot) => {
                        slot_share.store(slot, Ordering::Relaxed);
                        info!(slot, "Solana RPC: slot poll OK");
                    }
                    Err(e) => warn!(error = %e, "Solana RPC: slot poll failed — will retry"),
                }
            }
        });
    }

    // ── Metrics ────────────────────────────────────────────────────────────────
    let metrics = Arc::new(
        ApexMetrics::register().context("Failed to register Prometheus metrics")?,
    );
    info!("Prometheus metrics registered");

    // ── P&L tracking ──────────────────────────────────────────────────────────
    let pnl = AtomicPnL::new();
    let mut session_stats = SessionStats::new();
    info!("P&L tracker initialised (session start: {})", session_stats.start_time);

    // ── Self-Optimizer ─────────────────────────────────────────────────────────
    let mut self_optimizer = SelfOptimizer::new(
        TradingParams::new(
            config.slippage_bps,
            config.min_profit_lamports,
            40,
        ),
        config.auto_optimize,
    );
    info!(
        auto_optimize      = config.auto_optimize,
        initial_slippage   = config.slippage_bps,
        initial_min_profit = config.min_profit_lamports,
        "Self-optimizer initialised"
    );

    // ── Execution capital ──────────────────────────────────────────────────────
    // Jupiter Ultra returns the executable swap transaction for the operator wallet.
    if config.flash_loan_enabled {
        info!(
            "Flash loan flag is enabled, but live trade construction is delegated to Jupiter Ultra orders"
        );
    }

    // ── Risk subsystem ─────────────────────────────────────────────────────────
    let circuit_breaker = CircuitBreaker::new(
        config.circuit_breaker_consecutive_losses,
        config.circuit_breaker_threshold_lamports,
    );
    metrics.circuit_breaker_state.set(1.0);

    let anomaly_detector = Arc::new(Mutex::new(AnomalyDetector::new(200)));

    // ── Core engine ────────────────────────────────────────────────────────────
    let mut matrix_builder = MatrixBuilder::new();
    let strategy = Arc::new(
        ArbitrageStrategy::new(
            config.max_hops,
            config.min_profit_lamports,
            0.30,
            config.max_position_lamports,
        )
        .context("Failed to initialise arbitrage strategy")?,
    );

    // ── Safety layer ───────────────────────────────────────────────────────────
    let pre_sim = PreSimulator::new(config.min_profit_lamports);

    // ── Jito handler ───────────────────────────────────────────────────────────
    let flash_keypair: Option<Arc<ApexKeypair>>;

    let jito = {
        info!("Jito: LIVE mode — loading keypair from {}", config.keypair_path);
        let keypair = ApexKeypair::load(&config.keypair_path).with_context(|| {
            format!("Failed to load keypair from {} — ensure the file exists", config.keypair_path)
        })?;

        let rpc = jito_handler::SolanaRpcClient::new(&config.http_rpc_url)?;
        let bal = rpc.get_balance(&keypair.pubkey_b58).await?;
        info!(
            stage = "[WALLET OK]",
            pubkey  = %keypair.pubkey_b58,
            balance = format!("{:.9} SOL ({} lamports)", bal as f64 / 1e9, bal),
            "Operator wallet balance loaded"
        );
        enforce_wallet_balance(bal, config.jito_tip_lamports)?;

        let health = rpc.get_account_health(&keypair.pubkey_b58).await?;
        enforce_system_wallet(&health.owner, health.data_len)?;

        let fkp = Some(Arc::new(ApexKeypair::load(&config.keypair_path)?));
        flash_keypair = fkp;

        JitoBundleHandler::new_live(
            config.jito_url.clone(),
            &config.http_rpc_url,
            keypair,
        )
        .context("Failed to initialise live Jito handler")?
    };

    // ── Jupiter Ultra API client ───────────────────────────────────────────────
    let jup_ultra_client = build_ultra_client()
        .context("Failed to build Jupiter Ultra HTTP client")?;
    info!(
        endpoint_order = "https://api.jup.ag/ultra/v1/order",
        has_key        = config.jupiter_api_key.is_some(),
        "Jupiter Ultra client ready"
    );

    // ── Hot-path RPC client — created ONCE and reused across all trade cycles ───
    // Creating a new reqwest::Client per trade destroys TCP connection pooling
    // and adds ~5ms of reconnection latency on every execution cycle.
    let hot_rpc = Arc::new(
        SolanaRpcClient::new(&config.http_rpc_url)
            .context("Failed to build hot-path RPC client")?,
    );
    info!(url = %config.http_rpc_url, "Hot-path RPC client initialised (shared across trade cycles)");

    validate_all_connections(&config, &jito).await;

    if config.simulation_only {
        warn!(
            stage = "[SEND TX]",
            simulation_only = true,
            "Forced startup transaction skipped because simulation-only mode is enabled"
        );
    } else if let Some(kp) = flash_keypair.as_ref() {
        force_startup_transaction(hot_rpc.as_ref(), kp.as_ref()).await;
    } else {
        error!(
            stage = "[ERROR]",
            keypair_path = %config.keypair_path,
            "Forced startup transaction cannot run because the live signing keypair was not loaded"
        );
    }

    // ── Ingress streams ────────────────────────────────────────────────────────
    // Priority: Helius (PRIMARY) → Alchemy (FALLBACK). Mock mode is disabled.
    let ingress_source: &str;
    let mut shred_rx: Receiver<ShredEvent> =
        if let Some(ref helius_key) = config.helius_api_key {
            if !helius_key.is_empty() {
                ingress_source = "HELIUS (PRIMARY)";
                info!(
                    endpoint     = "wss://mainnet.helius-rpc.com",
                    dex_programs = ingress::DEX_PROGRAMS.len(),
                    "LIVE DATA SOURCE: HELIUS — connecting to primary WebSocket stream"
                );
                HeliusTransactionStream::spawn(helius_key.clone())
            } else {
                anyhow::bail!("HELIUS_API_KEY is empty; live execution does not allow mock ingress")
            }
        } else if let Some(ref alchemy_key) = config.alchemy_api_key {
            if !alchemy_key.is_empty() {
                ingress_source = "ALCHEMY (FALLBACK)";
                info!(
                    endpoint     = "wss://solana-mainnet.g.alchemy.com",
                    dex_programs = ingress::DEX_PROGRAMS.len(),
                    "LIVE DATA SOURCE: ALCHEMY (FALLBACK) — no Helius key, using Alchemy"
                );
                AlchemyTransactionStream::spawn(alchemy_key.clone())
            } else {
                anyhow::bail!("ALCHEMY_API_KEY is empty; live execution does not allow mock ingress")
            }
        } else {
            anyhow::bail!("HELIUS_API_KEY or ALCHEMY_API_KEY is required; live execution does not allow mock ingress")
        };

    info!(ingress = ingress_source, "Ingress stream configured");

    // ── Self-healing Jupiter Price Monitor ────────────────────────────────────
    info!(
        tokens     = ingress::TOKENS.len(),
        poll_ms    = 1500,
        endpoints  = 3,
        has_key    = config.jupiter_api_key.is_some(),
        "Starting self-healing Jupiter price monitor (tries all known endpoints)"
    );
    let mut jupiter_rx = JupiterMonitor::spawn_with_key(config.jupiter_api_key.clone());

    info!(
        "All subsystems initialised — entering LIVE hot loop"
    );
    info!(
        mode            = if config.simulation_only { "SIMULATION" } else { "LIVE TRADING" },
        ingress         = ingress_source,
        flash_loans     = config.flash_loan_enabled,
        min_profit      = config.min_profit_lamports,
        "System ready"
    );

    let mut live_edges: Option<Vec<common::types::MarketEdge>> = None;
    let mut live_prices_ever_received = false;
    let mut iteration: u64 = 0;
    let mut last_stats_report = std::time::Instant::now();
    const STATS_REPORT_INTERVAL_SECS: u64 = 60;

    // ── Adaptive rate limiter — replaces static 1200ms cooldown ──────────────
    // Learns from Jito outcomes: backs off on 429s, tightens on successes.
    // Dead-market detection: 10 consecutive 429s → pause for 5–15 seconds.
    let mut adaptive_cooldown = AdaptiveCooldown::new();

    // Deduplication set — prevents sending the same arbitrage path twice.
    // Bounded to 64 entries; cleared when full.
    let mut submitted_path_hashes: std::collections::HashSet<u64> =
        std::collections::HashSet::new();

    // ── Hot loop ───────────────────────────────────────────────────────────────
    loop {
        // ── Sync real slot from background poller ──────────────────────────
        {
            let slot = current_slot.load(Ordering::Relaxed);
            if slot > 0 {
                matrix_builder.set_slot(slot);
            }
        }

        // ── Absorb Jupiter price updates ───────────────────────────────────
        while let Ok(edges) = jupiter_rx.try_recv() {
            if !live_prices_ever_received {
                live_prices_ever_received = true;
                info!(
                    edges          = edges.len(),
                    source         = "JUPITER/LIVE",
                    "★ FIRST LIVE PRICES RECEIVED — execution pipeline UNBLOCKED (live_edges gate cleared)"
                );
            } else {
                info!(
                    edges  = edges.len(),
                    source = "JUPITER/LIVE",
                    "LIVE DATA SOURCE: JUPITER — price matrix updated"
                );
            }
            live_edges = Some(edges);
        }

        // ── Process shred/transaction event ───────────────────────────────
        let t_hot_start = std::time::Instant::now();

        if let Ok(shred) = shred_rx.try_recv() {
            if !filter_accepts(&shred) {
                tokio::task::yield_now().await;
                continue;
            }

            iteration += 1;
            metrics.paths_evaluated.inc();

            // ── Circuit breaker ────────────────────────────────────────────
            if circuit_breaker.check_allow_trade().is_err() {
                warn!(
                    iteration,
                    pnl_sol = pnl.total_sol(),
                    "Circuit breaker OPEN — halting trades temporarily"
                );
                metrics.circuit_breaker_state.set(0.0);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }

            // ── Select edge source ─────────────────────────────────────────
            // Use live prices if available; warn clearly when falling back to mock
            let (edges_ref, edge_source): (&[common::types::MarketEdge], &str) =
                match live_edges.as_deref() {
                    Some(live) if !live.is_empty() => (live, "JUPITER/LIVE"),
                    _ => {
                        // No live prices yet — skip this iteration and wait.
                        // Gate clears as soon as JupiterMonitor delivers its first batch.
                        if iteration % 50 == 0 {
                            warn!(
                                iteration,
                                live_prices_ever_received,
                                "BLOCKED: no live price data — waiting for Jupiter Price API \
                                 (https://api.jup.ag/price/v3). Execution begins as soon as prices arrive."
                            );
                        }
                        tokio::task::yield_now().await;
                        continue;
                    }
                };

            // ── Build price matrix ─────────────────────────────────────────
            let t_matrix = std::time::Instant::now();
            let matrix = matrix_builder.build(edges_ref);
            let matrix_us = t_matrix.elapsed().as_micros();

            // ── Strategy evaluation ────────────────────────────────────────
            let t_rich = std::time::Instant::now();
            let matrix_clone = matrix.clone();
            let strategy_arc = strategy.clone();
            let approved_trades = tokio::task::spawn_blocking(move || {
                strategy_arc.evaluate(&matrix_clone)
            })
            .await
            .unwrap_or_default();
            let rich_us = t_rich.elapsed().as_micros();

            let n_approved = approved_trades.len();
            metrics.paths_profitable.inc_by(n_approved as f64);

            // ── Pipeline probe: log strategy results periodically ────────────
            if n_approved == 0 {
                if iteration % 100 == 0 {
                    info!(
                        iteration,
                        matrix_μs  = matrix_us,
                        evaluate_μs = rich_us,
                        edges      = edges_ref.len(),
                        source     = edge_source,
                        "PIPELINE STAGE 2: strategy found 0 approved trades this cycle"
                    );
                }
            } else {
                info!(
                    iteration,
                    n_approved,
                    matrix_μs  = matrix_us,
                    evaluate_μs = rich_us,
                    source     = edge_source,
                    "PIPELINE STAGE 2: strategy found {} approved trade(s) — selecting best",
                    n_approved
                );
            }

            // ── Execution queue: pick only the BEST opportunity per cycle ─────
            // Sending multiple bundles at once causes Jito 429 errors (1 req/sec limit).
            // We select the highest expected-profit trade and drop the rest.
            if let Some(trade) = approved_trades
                .into_iter()
                .max_by_key(|t| t.path.expected_profit_lamports)
            {
                let dex_path: String = trade
                    .path
                    .edges
                    .iter()
                    .map(|e| format!("{}", e.dex))
                    .collect::<Vec<_>>()
                    .join(" → ");

                // ── Adaptive rate limiter — backs off on 429s, tightens on successes ──
                if !adaptive_cooldown.is_ready() {
                    if adaptive_cooldown.is_dead_market() {
                        warn!(
                            path = %dex_path,
                            cooldown_ms = adaptive_cooldown.current_ms(),
                            "Jito rate limiter: dead-market pause active — dropping this cycle"
                        );
                    }
                    tokio::task::yield_now().await;
                    continue;
                }

                // ── Deduplication — skip if the same path was recently submitted ──
                let path_hash = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    dex_path.hash(&mut h);
                    h.finish()
                };
                if submitted_path_hashes.contains(&path_hash) {
                    warn!(
                        path = %dex_path,
                        "Dedup: path already submitted this window — skipping"
                    );
                    tokio::task::yield_now().await;
                    continue;
                }

                info!(
                    stage = "[OPPORTUNITY DETECTED]",
                    iteration,
                    hops       = trade.path.edges.len(),
                    profit_est = trade.path.expected_profit_lamports,
                    confidence = format!("{:.3}", trade.path.gnn_confidence),
                    position   = trade.position_lamports,
                    path       = %dex_path,
                    source     = edge_source,
                    matrix_μs  = matrix_us,
                    rich_μs    = rich_us,
                    "Arbitrage opportunity detected — best of cycle"
                );

                // ── Pre-simulation ─────────────────────────────────────────
                let hops: Vec<(u64, u64, u16, f64)> = trade
                    .instructions
                    .iter()
                    .zip(trade.path.edges.iter())
                    .map(|(instr, edge)| {
                        let fee = dex_fee_bps(&edge.dex.to_string());
                        let log_w: f64 = edge
                            .log_weight
                            .to_string()
                            .parse()
                            .unwrap_or(0.0);
                        let exchange_rate = (-log_w).exp();
                        (trade.position_lamports, instr.min_out_lamports, fee, exchange_rate)
                    })
                    .collect();

                let active_min_profit = self_optimizer.params().min_profit_lamports;

                let t_sim = std::time::Instant::now();
                info!(
                    stage = "[SIMULATION START]",
                    iteration,
                    position_lamports = trade.position_lamports,
                    hops = hops.len(),
                    min_required = active_min_profit,
                    "Pre-simulation started"
                );
                let sim_result = pre_sim.simulate_swap(
                    trade.position_lamports,
                    &hops,
                    active_min_profit,
                );
                let sim_us = t_sim.elapsed().as_micros();

                let sim_ok = sim_result.is_profitable(active_min_profit);
                info!(
                    stage = "[SIMULATION RESULT]",
                    iteration,
                    ok = sim_ok,
                    expected_profit_lamports = sim_result.expected_profit_lamports,
                    error = ?sim_result.error,
                    elapsed_us = sim_us,
                    "Pre-simulation completed"
                );
                self_optimizer.record_simulation(sim_ok);

                if !sim_ok {
                    // Only skip if min_profit > 0 (when min_profit is 0, always proceed)
                    if active_min_profit > 0 {
                        warn!(
                            error        = ?sim_result.error,
                            sim_profit   = sim_result.expected_profit_lamports,
                            min_required = active_min_profit,
                            "PRE-SIM REJECT — trade below profit threshold"
                        );
                        continue;
                    }
                }

                // ── Anomaly check ──────────────────────────────────────────
                let is_anomaly = {
                    let mut detector = anomaly_detector.lock().await;
                    detector.observe(sim_result.expected_profit_lamports as i64)
                };

                if is_anomaly {
                    warn!(
                        profit = sim_result.expected_profit_lamports,
                        "Anomaly detected — skipping trade (unusual profit size)"
                    );
                    continue;
                }

                // ── Atomic revert guard ────────────────────────────────────
                let guard_tag = format!("trade_{iteration}");
                let guard = AtomicRevertGuard::new(trade.position_lamports, guard_tag);

                // ── Execute ────────────────────────────────────────────────
                if config.simulation_only {
                    let profit = sim_result.expected_profit_lamports as i64;
                    pnl.add(profit);
                    session_stats.record_trade(profit);
                    metrics.total_profit_lamports.add(profit as f64);
                    self_optimizer.record_trade(profit);

                    let record = make_record(
                        iteration,
                        trade.path.edges.len(),
                        trade.position_lamports,
                        profit,
                        trade.path.gnn_confidence,
                        None,
                        true,
                        &dex_path,
                    );
                    record.log_summary();
                    guard.commit();
                } else {
                    let operator_pubkey: String = flash_keypair
                        .as_ref()
                        .map(|kp| kp.pubkey_b58.clone())
                        .unwrap_or_default();

                    if operator_pubkey.is_empty() {
                        warn!(
                            iteration,
                            keypair_path = %config.keypair_path,
                            "EXECUTION BLOCKED: no operator keypair (flash_keypair is None). \
                             Ensure live-key.json exists. Skipping."
                        );
                        continue;
                    }

                    let input_mint: Option<String> = trade.path.edges.first()
                        .map(|e| bs58::encode(e.from.0).into_string());
                    let output_mint: Option<String> = trade.path.edges.get(1)
                        .map(|e| bs58::encode(e.to.0).into_string())
                        .or_else(|| trade.path.edges.last()
                            .map(|e| bs58::encode(e.to.0).into_string()));

                    let (input_mint, output_mint) = match (input_mint, output_mint) {
                        (Some(a), Some(b)) if a != b => (a, b),
                        _ => {
                            warn!(iteration, "Could not extract valid mints from arb path");
                            continue;
                        }
                    };

                    let fkp = match flash_keypair.as_ref() {
                        Some(k) => k,
                        None => {
                            warn!(iteration, "No signing keypair — skipping trade");
                            continue;
                        }
                    };

                    let wallet_balance = match hot_rpc.get_balance(&operator_pubkey).await {
                        Ok(balance) => balance,
                        Err(e) => {
                            warn!(stage = "[ERROR]", error = %e, "Wallet balance check failed — skipping cycle");
                            continue;
                        }
                    };
                    if let Err(e) = enforce_wallet_balance(wallet_balance, config.jito_tip_lamports) {
                        warn!(stage = "[ERROR]", error = %e, wallet_balance, "Wallet safety gate failed");
                        continue;
                    }

                    info!(
                        stage = "[BUILD TX]",
                        iteration,
                        input_mint = %input_mint,
                        output_mint = %output_mint,
                        amount = trade.position_lamports,
                        operator = %operator_pubkey,
                        "Requesting Jupiter Ultra order"
                    );
                    let jupiter_key = match config.jupiter_api_key.as_deref() {
                        Some(key) => key,
                        None => {
                            warn!(stage = "[ERROR]", "JUPITER_API_KEY missing — skipping cycle");
                            continue;
                        }
                    };
                    let ultra_order = match request_ultra_order(
                        &jup_ultra_client,
                        &input_mint,
                        &output_mint,
                        trade.position_lamports,
                        &operator_pubkey,
                        jupiter_key,
                    ).await {
                        Ok(order) => order,
                        Err(e) => {
                            warn!(
                                stage = "[ERROR]",
                                error = %e,
                                iteration,
                                "Jupiter Ultra order failed — stopping this cycle"
                            );
                            let is_rate_limit = e.to_string().contains("429")
                                || e.to_string().to_lowercase().contains("rate");
                            if is_rate_limit {
                                adaptive_cooldown.record_outcome(SubmitOutcome::RateLimit);
                            } else {
                                adaptive_cooldown.record_outcome(SubmitOutcome::Failure);
                            }
                            continue;
                        }
                    };

                    let required_lamports = ultra_order
                        .required_fee_lamports()
                        .saturating_add(config.jito_tip_lamports)
                        .saturating_add(10_000);
                    if wallet_balance < required_lamports {
                        warn!(
                            stage = "[ERROR]",
                            wallet_balance,
                            required_lamports,
                            "Wallet balance insufficient for Ultra fees plus Jito tip — skipping cycle"
                        );
                        continue;
                    }

                    info!(
                        stage = "[SWAP BUILT]",
                        iteration,
                        request_id = %ultra_order.request_id,
                        tx_b64_len = ultra_order.transaction_b64.len(),
                        required_lamports,
                        "Jupiter Ultra swap transaction received"
                    );

                    match hot_rpc.simulate_transaction(&ultra_order.transaction_b64).await {
                        Ok(true) => {
                            info!(
                                stage = "[SIMULATION PASSED]",
                                iteration,
                                path = %dex_path,
                                "Ultra transaction simulation passed"
                            );
                        }
                        Ok(false) => {
                            warn!(
                                stage = "[ERROR]",
                                iteration,
                                path = %dex_path,
                                "Ultra transaction simulation failed — skipping cycle"
                            );
                            adaptive_cooldown.record_outcome(SubmitOutcome::Failure);
                            continue;
                        }
                        Err(e) => {
                            warn!(
                                stage = "[ERROR]",
                                error = %e,
                                path = %dex_path,
                                "Ultra transaction simulation RPC error — skipping cycle"
                            );
                            adaptive_cooldown.record_outcome(SubmitOutcome::Failure);
                            continue;
                        }
                    }

                    let signed = match attach_jito_tip_and_sign(
                        &ultra_order.transaction_b64,
                        fkp,
                        &config.jito_tip_account,
                        config.jito_tip_lamports,
                    ) {
                        Ok(signed) => signed,
                        Err(e) => {
                            warn!(stage = "[ERROR]", error = %e, "Failed to attach Jito tip and sign transaction");
                            adaptive_cooldown.record_outcome(SubmitOutcome::Failure);
                            continue;
                        }
                    };

                    info!(
                        stage = "[JITO TIP ATTACHED]",
                        iteration,
                        tip_lamports = config.jito_tip_lamports,
                        tip_account = %config.jito_tip_account,
                        signature = %signed.signature,
                        tx_bytes = signed.tx_bytes.len(),
                        "Jito tip instruction attached and transaction signed"
                    );

                    let signed_b64 = base64::engine::general_purpose::STANDARD.encode(&signed.tx_bytes);
                    let sent_signature = match hot_rpc.send_transaction_base64(&signed_b64).await {
                        Ok(sig) => {
                            info!(
                                stage = "[TRANSACTION SENT]",
                                iteration,
                                signature = %sig,
                                tx_bytes = signed.tx_bytes.len(),
                                "Signed Ultra transaction sent to Solana RPC"
                            );
                            sig
                        }
                        Err(e) => {
                            warn!(stage = "[ERROR]", error = %e, "Solana sendTransaction failed");
                            adaptive_cooldown.record_outcome(SubmitOutcome::Failure);
                            continue;
                        }
                    };

                    match hot_rpc.confirm_transaction(&sent_signature, 24, 500).await {
                        Ok(status) => {
                            info!(
                                stage = "[CONFIRMED]",
                                iteration,
                                signature = %sent_signature,
                                slot = status.slot,
                                confirmations = ?status.confirmations,
                                confirmation_status = ?status.confirmation_status,
                                "Transaction confirmed on-chain"
                            );
                            let profit = ultra_order.expected_profit_lamports();
                            pnl.add(profit);
                            session_stats.record_trade(profit);
                            metrics.bundles_submitted.inc();
                            metrics.total_profit_lamports.add(profit as f64);
                            circuit_breaker.record_trade(profit).ok();
                            self_optimizer.record_trade(profit);
                            adaptive_cooldown.record_outcome(SubmitOutcome::Success);

                            if submitted_path_hashes.len() >= 64 {
                                submitted_path_hashes.clear();
                            }
                            submitted_path_hashes.insert(path_hash);

                            let record = make_record(
                                iteration,
                                trade.path.edges.len(),
                                trade.position_lamports,
                                profit,
                                trade.path.gnn_confidence,
                                None,
                                false,
                                &dex_path,
                            );
                            record.log_summary();
                            guard.commit();
                        }
                        Err(e) => {
                            warn!(stage = "[ERROR]", error = %e, signature = %sent_signature, "Transaction confirmation failed");
                            adaptive_cooldown.record_outcome(SubmitOutcome::Failure);
                            self_optimizer.record_trade(-1000);
                        }
                    }
                }
            }

            // ── Self-optimizer cycle ───────────────────────────────────────
            let _updated_params = self_optimizer.maybe_optimize();
            metrics.observe_hot_path(t_hot_start);
        }

        // ── Periodic session stats ─────────────────────────────────────────
        if last_stats_report.elapsed().as_secs() >= STATS_REPORT_INTERVAL_SECS {
            session_stats.log_summary();
            let opt_params = self_optimizer.params();
            let (total_sub, total_ok, total_429, total_fail) = adaptive_cooldown.stats();
            info!(
                iterations         = iteration,
                live_price_active  = live_edges.is_some(),
                current_slot       = current_slot.load(Ordering::Relaxed),
                pnl_lamports       = pnl.total_lamports(),
                pnl_sol            = format!("{:+.9}", pnl.total_sol()),
                current_slippage   = opt_params.slippage_bps,
                current_min_profit = opt_params.min_profit_lamports,
                jito_tip_lamports  = config.jito_tip_lamports,
                cooldown_ms        = adaptive_cooldown.current_ms(),
                bundles_submitted  = total_sub,
                bundles_accepted   = total_ok,
                bundles_rate_limit = total_429,
                bundles_failed     = total_fail,
                ingress_source     = ingress_source,
                "Periodic status — LIVE TRADING ENGINE"
            );
            last_stats_report = std::time::Instant::now();
        }

        tokio::task::yield_now().await;
    }
}

fn validate_required_environment(config: &ApexConfig) -> Result<()> {
    if config.simulation_only {
        anyhow::bail!("APEX_SIMULATION_ONLY=true is not allowed for live Ultra execution");
    }
    if config.jupiter_api_key.as_ref().map(|k| k.trim().is_empty()).unwrap_or(true) {
        anyhow::bail!("JUPITER_API_KEY is required for Jupiter Ultra API");
    }
    if config.helius_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false) == false
        && config.alchemy_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false) == false
    {
        anyhow::bail!("HELIUS_API_KEY or ALCHEMY_API_KEY is required; mock execution is disabled");
    }
    if config.jito_tip_account.contains('<') || config.jito_tip_account.trim().is_empty() {
        anyhow::bail!("JITO_TIP_ACCOUNT must be a real Jito tip account pubkey");
    }
    let decoded = bs58::decode(&config.jito_tip_account)
        .into_vec()
        .map_err(|e| anyhow::anyhow!("JITO_TIP_ACCOUNT is not valid base58: {e}"))?;
    if decoded.len() != 32 {
        anyhow::bail!("JITO_TIP_ACCOUNT must decode to 32 bytes, got {}", decoded.len());
    }
    if config.jito_tip_lamports < 1_000 {
        anyhow::bail!("JITO_TIP_LAMPORTS must be at least 1000 lamports");
    }
    info!(
        stage = "[CONFIG OK]",
        jupiter_api = "configured",
        live_ingress = "configured",
        jito_tip_account = %config.jito_tip_account,
        jito_tip_lamports = config.jito_tip_lamports,
        "Required live environment variables validated"
    );
    Ok(())
}

fn enforce_wallet_balance(balance_lamports: u64, tip_lamports: u64) -> Result<()> {
    let minimum = 10_000u64.saturating_add(tip_lamports).saturating_add(5_000);
    if balance_lamports < minimum {
        anyhow::bail!(
            "wallet balance {balance_lamports} lamports is insufficient for fees plus Jito tip; minimum {minimum}"
        );
    }
    Ok(())
}

fn enforce_system_wallet(owner: &str, data_len: usize) -> Result<()> {
    if owner != "11111111111111111111111111111111" || data_len != 0 {
        anyhow::bail!(
            "operator wallet is not a plain system account (owner={owner}, data_len={data_len}); Jito SystemProgram tip transfers require a system-owned fee wallet"
        );
    }
    info!(
        stage = "[WALLET OK]",
        owner,
        data_len,
        "Operator wallet is valid for SystemProgram Jito tip transfers"
    );
    Ok(())
}

// ─── Lightweight packet filter ────────────────────────────────────────────────
#[inline(always)]
fn filter_accepts(shred: &ShredEvent) -> bool {
    !shred.data.is_empty() && shred.data.len() <= 65536
}

async fn validate_all_connections(config: &ApexConfig, jito: &JitoBundleHandler) {
    validate_rpc_connection("Configured Solana RPC", &config.http_rpc_url).await;

    if let Some(key) = config.helius_api_key.as_ref().filter(|k| !k.is_empty()) {
        let url = format!("https://mainnet.helius-rpc.com/?api-key={key}");
        validate_rpc_connection("Helius RPC", &url).await;
    } else {
        warn!(stage = "[ERROR]", service = "Helius RPC", "HELIUS_API_KEY is not configured");
    }

    if let Some(key) = config.alchemy_api_key.as_ref().filter(|k| !k.is_empty()) {
        let url = format!("https://solana-mainnet.g.alchemy.com/v2/{key}");
        validate_rpc_connection("Alchemy RPC", &url).await;
    } else {
        warn!(stage = "[ERROR]", service = "Alchemy RPC", "ALCHEMY_API_KEY is not configured");
    }

    let t = std::time::Instant::now();
    match jito.get_tip_accounts().await {
        accounts if !accounts.is_empty() => {
            info!(
                stage = "[RPC CONNECTED]",
                service = "Jito endpoint",
                tip_accounts = accounts.len(),
                elapsed_ms = t.elapsed().as_millis(),
                "Jito endpoint validation succeeded"
            );
        }
        _ => {
            error!(
                stage = "[ERROR]",
                service = "Jito endpoint",
                elapsed_ms = t.elapsed().as_millis(),
                "Jito endpoint validation failed"
            );
        }
    }
}

async fn validate_rpc_connection(label: &str, url: &str) {
    let t = std::time::Instant::now();
    match SolanaRpcClient::new(url) {
        Ok(client) => match client.get_slot().await {
            Ok(slot) => {
                info!(
                    stage = "[RPC CONNECTED]",
                    service = label,
                    slot,
                    elapsed_ms = t.elapsed().as_millis(),
                    "RPC validation succeeded"
                );
            }
            Err(e) => {
                error!(
                    stage = "[ERROR]",
                    service = label,
                    url = %url,
                    error = %e,
                    elapsed_ms = t.elapsed().as_millis(),
                    "RPC validation failed"
                );
            }
        },
        Err(e) => {
            error!(
                stage = "[ERROR]",
                service = label,
                url = %url,
                error = %e,
                elapsed_ms = t.elapsed().as_millis(),
                "RPC client construction failed"
            );
        }
    }
}

async fn force_startup_transaction(rpc: &SolanaRpcClient, keypair: &ApexKeypair) {
    const MAX_ATTEMPTS: u32 = 3;
    const MEMO_PROGRAM: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

    for attempt in 1..=MAX_ATTEMPTS {
        let trace = std::time::Instant::now();
        info!(
            stage = "[BUILD TX]",
            attempt,
            from = %keypair.pubkey_b58,
            memo_program = MEMO_PROGRAM,
            "Forced startup proof transaction build started"
        );

        let blockhash = match rpc.get_latest_blockhash().await {
            Ok(bh) => bh.blockhash,
            Err(e) => {
                error!(
                    stage = "[ERROR]",
                    attempt,
                    error = %e,
                    elapsed_ms = trace.elapsed().as_millis(),
                    "Forced startup transaction could not fetch blockhash"
                );
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                continue;
            }
        };

        let (tx_bytes, signature) = match build_startup_memo_tx(
            keypair,
            &blockhash,
        ) {
            Ok(tx) => tx,
            Err(e) => {
                error!(
                    stage = "[ERROR]",
                    attempt,
                    error = %e,
                    elapsed_ms = trace.elapsed().as_millis(),
                    "Forced startup transaction build failed"
                );
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                continue;
            }
        };

        info!(
            stage = "[SIGN TX]",
            attempt,
            signature = %signature,
            tx_bytes = tx_bytes.len(),
            elapsed_ms = trace.elapsed().as_millis(),
            "Forced startup proof transaction signed"
        );

        let tx_b64 = base64::engine::general_purpose::STANDARD.encode(&tx_bytes);
        let sim_t = std::time::Instant::now();
        info!(
            stage = "[SIMULATION START]",
            attempt,
            signature = %signature,
            tx_bytes = tx_bytes.len(),
            "Forced startup proof transaction simulation started"
        );

        match rpc.simulate_transaction(&tx_b64).await {
            Ok(true) => {
                info!(
                    stage = "[SIMULATION RESULT]",
                    attempt,
                    signature = %signature,
                    ok = true,
                    elapsed_ms = sim_t.elapsed().as_millis(),
                    "Forced startup proof transaction simulation passed"
                );
            }
            Ok(false) => {
                error!(
                    stage = "[SIMULATION RESULT]",
                    attempt,
                    signature = %signature,
                    ok = false,
                    elapsed_ms = sim_t.elapsed().as_millis(),
                    "Forced startup proof transaction simulation rejected"
                );
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                continue;
            }
            Err(e) => {
                error!(
                    stage = "[ERROR]",
                    attempt,
                    signature = %signature,
                    error = %e,
                    elapsed_ms = sim_t.elapsed().as_millis(),
                    "Forced startup proof transaction simulation failed"
                );
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                continue;
            }
        }

        let send_t = std::time::Instant::now();
        info!(
            stage = "[SEND TX]",
            attempt,
            signature = %signature,
            tx_bytes = tx_bytes.len(),
            "Forced startup proof transaction send started"
        );

        match rpc.send_transaction_base64(&tx_b64).await {
            Ok(sent_signature) => {
                info!(
                    stage = "[SEND TX]",
                    attempt,
                    signature = %sent_signature,
                    elapsed_ms = send_t.elapsed().as_millis(),
                    "Forced startup proof transaction sent"
                );

                let confirm_t = std::time::Instant::now();
                match rpc.confirm_transaction(&sent_signature, 30, 500).await {
                    Ok(status) => {
                        info!(
                            stage = "[CONFIRMATION RESULT]",
                            attempt,
                            signature = %sent_signature,
                            slot = status.slot,
                            confirmations = ?status.confirmations,
                            confirmation_status = ?status.confirmation_status,
                            elapsed_ms = confirm_t.elapsed().as_millis(),
                            total_elapsed_ms = trace.elapsed().as_millis(),
                            "Forced startup proof transaction confirmed on-chain"
                        );
                        return;
                    }
                    Err(e) => {
                        error!(
                            stage = "[ERROR]",
                            attempt,
                            signature = %sent_signature,
                            error = %e,
                            elapsed_ms = confirm_t.elapsed().as_millis(),
                            "Forced startup proof transaction confirmation failed"
                        );
                    }
                }
            }
            Err(e) => {
                error!(
                    stage = "[ERROR]",
                    attempt,
                    signature = %signature,
                    error = %e,
                    elapsed_ms = send_t.elapsed().as_millis(),
                    "Forced startup proof transaction send failed"
                );
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
    }

    error!(
        stage = "[ERROR]",
        attempts = MAX_ATTEMPTS,
        "Forced startup proof transaction exhausted all retry attempts without confirmation"
    );
}

fn build_startup_memo_tx(
    keypair: &ApexKeypair,
    blockhash_b58: &str,
) -> anyhow::Result<(Vec<u8>, String)> {
    let memo_program_b58 = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
    let from_bytes = keypair.pubkey_bytes;
    let memo_program_bytes = bs58_to_32_checked(memo_program_b58)?;
    let blockhash_bytes = bs58_to_32_checked(blockhash_b58)?;
    let ix_data = format!("apex-mev startup proof {}", chrono::Utc::now().timestamp_millis())
        .into_bytes();

    let mut msg = Vec::new();
    msg.push(1_u8);
    msg.push(0_u8);
    msg.push(1_u8);
    msg.push(2_u8);
    msg.extend_from_slice(&from_bytes);
    msg.extend_from_slice(&memo_program_bytes);
    msg.extend_from_slice(&blockhash_bytes);
    msg.push(1_u8);
    msg.push(1_u8);
    msg.push(1_u8);
    msg.push(0_u8);
    msg.push(ix_data.len() as u8);
    msg.extend_from_slice(&ix_data);

    let sig = keypair.sign(&msg);
    if !keypair.verify(&msg, &sig) {
        anyhow::bail!("startup proof signature verification failed");
    }

    let mut tx = Vec::with_capacity(1 + 64 + msg.len());
    tx.push(1_u8);
    tx.extend_from_slice(&sig);
    tx.extend_from_slice(&msg);

    Ok((tx, bs58::encode(sig).into_string()))
}

fn bs58_to_32_checked(value: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = bs58::decode(value)
        .into_vec()
        .map_err(|e| anyhow::anyhow!("invalid base58 public key {value}: {e}"))?;
    if bytes.len() != 32 {
        anyhow::bail!("base58 public key {value} decoded to {} bytes, expected 32", bytes.len());
    }
    let mut out = [0_u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}
