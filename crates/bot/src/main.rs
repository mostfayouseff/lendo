// =============================================================================
// APEX-MEV Neural Core 3.0 — LIVE TRADING MODE
//
// INGRESS PRIORITY:
//   1. Helius WebSocket (PRIMARY)   — wss://mainnet.helius-rpc.com/?api-key=<KEY>
//   2. Alchemy WebSocket (FALLBACK) — wss://solana-mainnet.g.alchemy.com/v2/<KEY>
//   3. Mock stream (LAST RESORT)    — logged clearly, indicates no WS keys set
//
// PRICE DATA (self-healing):
//   1. Jupiter Price v3 — https://api.jup.ag/price/v3?ids=...  (PRIMARY)
//   2. CoinGecko public API                                     (FALLBACK)
//   3. Stale cache                                              (LAST RESORT)
//
// EXECUTION — Jupiter Swap V2 API:
//   GET /swap/v2/order — opportunity detection only (never used to build tx)
//   GET /swap/v2/build — real swap instructions, ALT resolution, Metis routing
//   Atomic VersionedTransaction v0: FlashBorrow + Swap1 + Swap2 + FlashRepay + Tip
//   Flash loans via Solend (0.09% fee) + Jito bundle submission
//
// FORBIDDEN ENDPOINTS:
//   /ultra/v1/*  |  /swap/v1/*  |  /swap-instructions  |  lite-api.jup.ag
//   POST /execute
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
    build_swap_v2_client, detect_opportunity, get_build_instructions,
    AlchemyTransactionStream, HeliusTransactionStream,
    JupiterMonitor, MockShredStream, MockYellowstoneStream,
};
use jito_handler::{
    build_atomic_flash_v0, select_random_tip_account, solend_repay_amount,
    AtomicAccountMeta, AtomicInstruction, ApexKeypair, JitoBundleHandler,
    SolanaRpcClient, TipCalculator,
};
use pnl::{make_record, AtomicPnL, SessionStats};
use risk_oracle::{AnomalyDetector, CircuitBreaker, SelfOptimizer, TradingParams};
use safety::{AtomicRevertGuard, PreSimulator};
use solana_program_apex::instruction::dex_fee_bps;
use std::collections::HashMap;
use std::sync::Arc;
use strategy::ArbitrageStrategy;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use tracing::{info, warn};

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

    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║   APEX-MEV Neural Core 3.0  — LIVE MAINNET TRADING          ║");
    info!("╚══════════════════════════════════════════════════════════════╝");

    // ── Configuration ─────────────────────────────────────────────────────────
    let config = ApexConfig::from_env()
        .context("Failed to load configuration — check env vars")?;

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
                info!(slot, url = %config.http_rpc_url, "Solana RPC: CONNECTED — getSlot OK");
            }
            Err(e) => {
                warn!(
                    error = %e,
                    url   = %config.http_rpc_url,
                    "Solana RPC: getSlot FAILED — will retry in background. Continuing."
                );
            }
        },
        Err(e) => {
            warn!(error = %e, "Solana RPC: client init failed — continuing anyway");
        }
    }

    // ── Background slot poller ─────────────────────────────────────────────────
    {
        let rpc_url = config.http_rpc_url.clone();
        tokio::spawn(async move {
            let rpc = match SolanaRpcClient::new(&rpc_url) {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Slot poller: could not build RPC client");
                    return;
                }
            };
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                match rpc.get_slot().await {
                    Ok(slot) => info!(slot, "Solana RPC: slot poll OK"),
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

    // ── Flash loans ────────────────────────────────────────────────────────────
    // Flash loan instructions are assembled inline per trade via flash_tx_v2.
    // Solend charges 0.09% (9 bps) per borrow; fee is calculated with solend_repay_amount().
    if config.flash_loan_enabled {
        info!(
            "Flash loans: ENABLED (Solend, 9bps fee) — assembled atomically per trade via flash_tx_v2"
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

    let jito = if config.simulation_only {
        info!("Jito: SIMULATION mode — bundles logged but NOT submitted");
        flash_keypair = None;
        JitoBundleHandler::new(config.jito_url.clone())
    } else {
        info!("Jito: LIVE mode — loading keypair from {}", config.keypair_path);
        let keypair = ApexKeypair::load(&config.keypair_path).with_context(|| {
            format!("Failed to load keypair from {} — ensure the file exists", config.keypair_path)
        })?;

        // Log wallet balance — informational only, NOT a gate
        if let Ok(rpc) = jito_handler::SolanaRpcClient::new(&config.http_rpc_url) {
            match rpc.get_balance(&keypair.pubkey_b58).await {
                Ok(bal) => {
                    info!(
                        pubkey  = %keypair.pubkey_b58,
                        balance = format!("{:.9} SOL ({} lamports)", bal as f64 / 1e9, bal),
                        "Operator wallet: balance logged — flash loans do NOT require pre-funded balance"
                    );
                    if bal < 5_000_000 {
                        warn!(
                            balance_lamports = bal,
                            "Operator wallet balance is low (< 0.005 SOL) — ensure enough for transaction fees. Flash loan capital is borrowed atomically."
                        );
                    }
                }
                Err(e) => warn!("Could not fetch operator balance: {e} — continuing"),
            }
        }

        // Load flash loan keypair
        let fkp = if config.flash_loan_enabled {
            match ApexKeypair::load(&config.keypair_path) {
                Ok(kp) => {
                    info!(pubkey = %kp.pubkey_b58, "Flash loan signing keypair loaded");
                    Some(Arc::new(kp))
                }
                Err(e) => {
                    warn!(error = %e, "Flash loan keypair load failed — flash loans disabled for this session");
                    None
                }
            }
        } else {
            None
        };
        flash_keypair = fkp;

        JitoBundleHandler::new_live(
            config.jito_url.clone(),
            &config.http_rpc_url,
            keypair,
        )
        .context("Failed to initialise live Jito handler")?
    };

    // ── Jupiter Swap V2 API client ─────────────────────────────────────────────
    // GET /swap/v2/order — detect_opportunity (detection only, never for tx building)
    // GET /swap/v2/build — get_build_instructions (real atomic swap instructions)
    //
    // FORBIDDEN: /ultra/v1/*  |  /swap/v1/*  |  /swap-instructions  |  lite-api.jup.ag
    let jup_v2_client = build_swap_v2_client()
        .context("Failed to build Jupiter Swap V2 HTTP client")?;
    let tip_calculator = TipCalculator::new();
    info!(
        endpoint_order = "https://api.jup.ag/swap/v2/order",
        endpoint_build = "https://api.jup.ag/swap/v2/build",
        has_key        = config.jupiter_api_key.is_some(),
        "Jupiter Swap V2 client ready (GET /order for detection, GET /build for execution)"
    );

    // ── Ingress streams ────────────────────────────────────────────────────────
    // Priority: Helius (PRIMARY) → Alchemy (FALLBACK) → Mock (LAST RESORT)
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
                ingress_source = "MOCK (no Helius key)";
                warn!("Helius API key is empty — using MockShredStream. Set HELIUS_API_KEY for live data.");
                MockShredStream::spawn(400)
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
                ingress_source = "MOCK (no WS keys)";
                warn!("No Helius or Alchemy API keys set — using MockShredStream. Set HELIUS_API_KEY for live data.");
                MockShredStream::spawn(400)
            }
        } else {
            ingress_source = "MOCK (no WS keys configured)";
            warn!(
                "HELIUS_API_KEY and ALCHEMY_API_KEY not set — using MockShredStream. \
                 Set HELIUS_API_KEY for live mainnet DEX event stream."
            );
            MockShredStream::spawn(400)
        };

    info!(ingress = ingress_source, "Ingress stream configured");

    let mut slot_rx = MockYellowstoneStream::spawn(2);

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
    let mut iteration: u64 = 0;
    let mut last_stats_report = std::time::Instant::now();
    const STATS_REPORT_INTERVAL_SECS: u64 = 60;

    // ── Jito rate limiter + execution queue state ─────────────────────────────
    // Jito block engine allows at most 1 bundle per second.
    // After any bundle submission we enforce a 1200ms cooldown before the next.
    let mut last_bundle_submit: Option<std::time::Instant> = None;
    const JITO_COOLDOWN_MS: u64 = 1_200;

    // Deduplication set — prevents sending the same arbitrage path twice.
    // Bounded to 64 entries; cleared when full.
    let mut submitted_path_hashes: std::collections::HashSet<u64> =
        std::collections::HashSet::new();

    // ── Hot loop ───────────────────────────────────────────────────────────────
    loop {
        // ── Absorb slot updates ────────────────────────────────────────────
        if let Ok(slot_update) = slot_rx.try_recv() {
            matrix_builder.set_slot(slot_update.slot);
        }

        // ── Absorb Jupiter price updates ───────────────────────────────────
        while let Ok(edges) = jupiter_rx.try_recv() {
            info!(
                edges  = edges.len(),
                source = "JUPITER/LIVE",
                "LIVE DATA SOURCE: JUPITER — price matrix updated"
            );
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
                        // No live prices yet — skip this iteration and wait
                        // We do NOT fall back to mock edges in live mode
                        if iteration % 500 == 0 {
                            warn!(
                                iteration,
                                "No live price data yet — waiting for Jupiter price monitor. \
                                 Bot will execute as soon as live prices arrive."
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

                // ── Jito rate limiter — enforce 1200ms cooldown between submissions
                if let Some(last) = last_bundle_submit {
                    let elapsed = last.elapsed();
                    let cooldown = std::time::Duration::from_millis(JITO_COOLDOWN_MS);
                    if elapsed < cooldown {
                        warn!(
                            remaining_ms = (cooldown - elapsed).as_millis(),
                            path = %dex_path,
                            "Jito rate limiter: cooldown active — dropping this cycle"
                        );
                        tokio::task::yield_now().await;
                        continue;
                    }
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
                let sim_result = pre_sim.simulate_swap(
                    trade.position_lamports,
                    &hops,
                    active_min_profit,
                );
                let _sim_us = t_sim.elapsed().as_micros();

                let sim_ok = sim_result.is_profitable(active_min_profit);
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
                    // ── LIVE TRADING: Jupiter Swap V2 atomic pipeline ──────────────────────
                    //
                    // Step 1: GET /order  — confirm profitability (detection only)
                    // Step 2: GET /build  — swap leg 1 instructions (SOL → token)
                    // Step 3: GET /build  — swap leg 2 instructions (token → SOL)
                    // Step 4: build_atomic_flash_v0 — assemble atomic VersionedTransaction v0
                    //         [ComputeBudget][FlashBorrow][Swap1][Swap2][FlashRepay][JitoTip]
                    // Step 5: Submit to Jito as MEV bundle
                    //
                    // FORBIDDEN: /ultra/v1/* | /swap/v1/* | /swap-instructions | lite-api.jup.ag

                    // ── Step 1: Extract mints from arb path ──────────────────────────────
                    // leg1: edge[0].from (SOL) → intermediate token
                    // leg2: intermediate token → edge[0].from (SOL)
                    let operator_pubkey: String = flash_keypair
                        .as_ref()
                        .map(|kp| kp.pubkey_b58.clone())
                        .unwrap_or_default();

                    if operator_pubkey.is_empty() {
                        warn!(iteration, "No operator keypair — skipping live trade");
                        continue;
                    }

                    let leg1_in: Option<String> = trade.path.edges.first()
                        .map(|e| bs58::encode(e.from.0).into_string());
                    let leg1_out: Option<String> = trade.path.edges.get(1)
                        .map(|e| bs58::encode(e.to.0).into_string())
                        .or_else(|| trade.path.edges.last()
                            .map(|e| bs58::encode(e.to.0).into_string()));

                    let (leg1_in, leg1_out) = match (leg1_in, leg1_out) {
                        (Some(a), Some(b)) if a != b => (a, b),
                        _ => {
                            warn!(iteration, "Could not extract valid mints from arb path");
                            continue;
                        }
                    };
                    let leg2_in  = leg1_out.clone(); // intermediate → SOL
                    let leg2_out = leg1_in.clone();  // back to SOL

                    let active_slippage = self_optimizer.params().slippage_bps;

                    // ── Step 2: GET /order — confirm profitability (detection only) ────────
                    let order_quote = match detect_opportunity(
                        &jup_v2_client,
                        &leg1_in,
                        &leg1_out,
                        trade.position_lamports,
                        active_slippage,
                        config.jupiter_api_key.as_deref(),
                    )
                    .await
                    {
                        Ok(q) => q,
                        Err(e) => {
                            warn!(error = %e, path = %dex_path, "GET /order failed — skipping");
                            continue;
                        }
                    };

                    let active_min_profit = self_optimizer.params().min_profit_lamports;
                    if !order_quote.is_profitable(active_min_profit) {
                        warn!(
                            profit       = order_quote.expected_profit_lamports(),
                            min_required = active_min_profit,
                            impact_pct   = order_quote.price_impact_pct,
                            "GET /order: opportunity not profitable — skipping"
                        );
                        continue;
                    }

                    let borrow_lamports     = trade.position_lamports;
                    let repay_lamports      = solend_repay_amount(borrow_lamports);
                    let expected_mid_amount = order_quote.out_amount;

                    // ── Step 3: GET /build — swap leg 1 (SOL → token) ────────────────────
                    let build1 = match get_build_instructions(
                        &jup_v2_client,
                        &config.http_rpc_url,
                        &leg1_in,
                        &leg1_out,
                        borrow_lamports,
                        &operator_pubkey,
                        active_slippage,
                        config.jupiter_api_key.as_deref(),
                    )
                    .await
                    {
                        Ok(b) => b,
                        Err(e) => {
                            warn!(error = %e, "GET /build leg1 failed — skipping");
                            continue;
                        }
                    };

                    // ── Step 4: GET /build — swap leg 2 (token → SOL) ────────────────────
                    let build2 = match get_build_instructions(
                        &jup_v2_client,
                        &config.http_rpc_url,
                        &leg2_in,
                        &leg2_out,
                        expected_mid_amount,
                        &operator_pubkey,
                        active_slippage,
                        config.jupiter_api_key.as_deref(),
                    )
                    .await
                    {
                        Ok(b) => b,
                        Err(e) => {
                            warn!(error = %e, "GET /build leg2 failed — skipping");
                            continue;
                        }
                    };

                    // ── Step 5: Convert V2 instructions to AtomicInstruction ──────────────
                    let compute_budget_ixs = match to_atomic_ixs(&build1.compute_budget_instructions) {
                        Ok(v) => v,
                        Err(e) => { warn!(error = %e, "compute_budget decode failed"); continue; }
                    };
                    let swap1_ixs = match to_atomic_ixs(&build1.swap_only_instructions()) {
                        Ok(v) => v,
                        Err(e) => { warn!(error = %e, "swap1 decode failed"); continue; }
                    };
                    let swap2_ixs = match to_atomic_ixs(&build2.swap_only_instructions()) {
                        Ok(v) => v,
                        Err(e) => { warn!(error = %e, "swap2 decode failed"); continue; }
                    };

                    // Merge ALT maps from both swap legs
                    let mut combined_alts: HashMap<String, Vec<String>> = build1.alt_map.clone();
                    for (k, v) in &build2.alt_map {
                        combined_alts.entry(k.clone()).or_insert_with(|| v.clone());
                    }

                    // ── Jito tip ─────────────────────────────────────────────────────────
                    let swap_tip_lamports = tip_calculator
                        .compute_tip(order_quote.expected_profit_lamports().max(0) as u64)
                        .unwrap_or(10_000);
                    let jito_tip_account = select_random_tip_account();

                    // ── Step 6: Fetch latest blockhash ───────────────────────────────────
                    let blockhash = match SolanaRpcClient::new(&config.http_rpc_url) {
                        Ok(rpc) => match rpc.get_latest_blockhash().await {
                            Ok(bh) => bh.blockhash,
                            Err(e) => {
                                warn!(error = %e, "Blockhash fetch failed — skipping");
                                continue;
                            }
                        },
                        Err(e) => {
                            warn!(error = %e, "RPC init failed — skipping");
                            continue;
                        }
                    };

                    // ── Step 7: Build atomic VersionedTransaction v0 ─────────────────────
                    let fkp = match flash_keypair.as_ref() {
                        Some(k) => k,
                        None => {
                            warn!(iteration, "No signing keypair — skipping trade");
                            continue;
                        }
                    };

                    let tx_bytes = match build_atomic_flash_v0(
                        fkp,
                        &blockhash,
                        borrow_lamports,
                        repay_lamports,
                        &compute_budget_ixs,
                        &swap1_ixs,
                        &swap2_ixs,
                        &combined_alts,
                        swap_tip_lamports,
                        &jito_tip_account,
                    ) {
                        Ok(b) => b,
                        Err(e) => {
                            warn!(error = %e, "build_atomic_flash_v0 failed — skipping");
                            continue;
                        }
                    };

                    if tx_bytes.is_empty() {
                        warn!(iteration, "Atomic tx builder returned empty bytes — skipping");
                        continue;
                    }

                    info!(
                        tx_bytes     = tx_bytes.len(),
                        borrow_sol   = format!("{:.6}", borrow_lamports as f64 / 1e9),
                        repay_sol    = format!("{:.6}", repay_lamports  as f64 / 1e9),
                        tip_lamports = swap_tip_lamports,
                        tip_account  = %jito_tip_account,
                        blockhash    = %blockhash,
                        path         = %dex_path,
                        "Atomic v0 transaction built — submitting to Jito"
                    );

                    // ── Step 8: Submit to Jito ───────────────────────────────────────────
                    let payloads = vec![tx_bytes];
                    let profit_for_jito = order_quote.expected_profit_lamports().max(0) as u64;

                    match jito.submit(payloads, profit_for_jito).await {
                        Ok(bundle) => {
                            let profit = order_quote.expected_profit_lamports();
                            pnl.add(profit);
                            session_stats.record_trade(profit);
                            metrics.bundles_submitted.inc();
                            metrics.total_profit_lamports.add(profit as f64);
                            circuit_breaker.record_trade(profit).ok();
                            self_optimizer.record_trade(profit);

                            last_bundle_submit = Some(std::time::Instant::now());
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
                                Some(bundle.id),
                                false,
                                &dex_path,
                            );
                            record.log_summary();
                            guard.commit();
                        }
                        Err(e) => {
                            warn!(
                                error = %e,
                                iter  = iteration,
                                "Bundle submission failed — not counting as trading loss"
                            );
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
            info!(
                iterations         = iteration,
                live_price_active  = live_edges.is_some(),
                pnl_lamports       = pnl.total_lamports(),
                pnl_sol            = format!("{:+.9}", pnl.total_sol()),
                current_slippage   = opt_params.slippage_bps,
                current_min_profit = opt_params.min_profit_lamports,
                current_tip_pct    = opt_params.tip_fraction_pct,
                ingress_source     = ingress_source,
                "Periodic status — LIVE TRADING ENGINE"
            );
            last_stats_report = std::time::Instant::now();
        }

        tokio::task::yield_now().await;
    }
}

// ─── Lightweight packet filter ────────────────────────────────────────────────
#[inline(always)]
fn filter_accepts(shred: &ShredEvent) -> bool {
    !shred.data.is_empty() && shred.data.len() <= 65536
}

// ─── Instruction conversion: V2Instruction → AtomicInstruction ───────────────
/// Convert a slice of Jupiter V2 API instructions into AtomicInstruction format
/// expected by `build_atomic_flash_v0`. Decodes instruction data from base64.
fn to_atomic_ixs(ixs: &[ingress::V2Instruction]) -> anyhow::Result<Vec<AtomicInstruction>> {
    ixs.iter()
        .map(|ix| {
            let data = base64::engine::general_purpose::STANDARD
                .decode(&ix.data)
                .map_err(|e| anyhow::anyhow!("base64 decode failed for {}: {e}", ix.program_id))?;
            Ok(AtomicInstruction {
                program_id: ix.program_id.clone(),
                accounts: ix.accounts.iter().map(|a| AtomicAccountMeta {
                    pubkey:      a.pubkey.clone(),
                    is_signer:   a.is_signer,
                    is_writable: a.is_writable,
                }).collect(),
                data,
            })
        })
        .collect()
}
