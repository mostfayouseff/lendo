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
// EXECUTION:
//   Jupiter Ultra API — real executable transactions for each detected arb hop
//   Flash loans via Solend + Jito bundle submission
//   No minimum balance restriction — flash loans borrow capital atomically
//   No minimum profit restriction — all viable arb paths attempted
//
// LOGGING:
//   "LIVE DATA SOURCE: HELIUS" — when Helius is active
//   "LIVE DATA SOURCE: ALCHEMY (FALLBACK)" — when Alchemy is active
//   "LIVE DATA SOURCE: JUPITER" — when Jupiter prices update
//   "JUPITER API FAILED - SEARCHING FOR ALTERNATIVE" — on API failure
//   "NEW API DISCOVERED AND VERIFIED" — on endpoint recovery
//   All wallet balances, trades, errors, and retries are logged
// =============================================================================

mod pnl;

use anyhow::{Context, Result};
use common::{ApexConfig, ApexMetrics};
use apex_core::MatrixBuilder;
use ingress::{
    build_signed_swap_transaction, build_swap_client, build_ultra_client,
    get_best_route_and_transaction, AlchemyTransactionStream, HeliusTransactionStream,
    JupiterMonitor, MockShredStream, MockYellowstoneStream,
};
use jito_handler::{
    build_flash_loan_tx, select_random_tip_account, ApexKeypair, JitoBundleHandler,
    SolanaRpcClient, TipCalculator,
};
use pnl::{make_record, AtomicPnL, SessionStats};
use risk_oracle::{AnomalyDetector, CircuitBreaker, SelfOptimizer, TradingParams};
use safety::{AtomicRevertGuard, PreSimulator};
use solana_program_apex::instruction::dex_fee_bps;
use std::sync::Arc;
use strategy::{ArbitrageStrategy, SolendFlashLoan};
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

    // ── Flash loan builder ─────────────────────────────────────────────────────
    let flash_loan = SolendFlashLoan::new_sol();
    if config.flash_loan_enabled {
        info!(
            "Flash loans: ENABLED (Solend, 9bps fee) — atomic borrow+swap+repay per trade"
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

    // ── Jupiter API clients ────────────────────────────────────────────────────
    // ultra_client  — /ultra/v1/order (PRIMARY: complete pre-built transaction)
    // swap_client   — /v6/quote + /v6/swap-instructions (SECONDARY: individual instructions)
    let ultra_client = build_ultra_client()
        .context("Failed to build Jupiter Ultra HTTP client")?;
    let swap_client = build_swap_client()
        .context("Failed to build Jupiter swap-instructions HTTP client")?;
    let tip_calculator = TipCalculator::new();
    info!(
        endpoint_ultra = "https://api.jup.ag/ultra/v1/order",
        endpoint_swap  = "https://api.jup.ag/v6/swap-instructions",
        has_key        = config.jupiter_api_key.is_some(),
        "Jupiter API clients ready — Ultra (primary) + swap-instructions (secondary)"
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

                // ── Flash loan plan ────────────────────────────────────────
                let flash_plan_for_live = if config.flash_loan_enabled {
                    let borrower_key = flash_keypair
                        .as_ref()
                        .map(|kp| kp.pubkey_bytes)
                        .unwrap_or([0u8; 32]);
                    match flash_loan.build_plan(trade.position_lamports, &borrower_key) {
                        Ok(plan) => {
                            SolendFlashLoan::check_viability(
                                &plan,
                                trade.path.expected_profit_lamports,
                            );
                            let viable = plan.is_viable(trade.path.expected_profit_lamports);
                            if config.simulation_only {
                                SolendFlashLoan::log_plan(&plan);
                            }
                            if viable { Some(plan) } else { None }
                        }
                        Err(e) => {
                            warn!(error = %e, "Flash loan plan failed — proceeding without flash loan");
                            None
                        }
                    }
                } else {
                    None
                };

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
                    // ── LIVE TRADING: build + sign + submit to Jito ────────
                    //
                    // Execution priority:
                    //   1. Jupiter Ultra API (/ultra/v1/order)
                    //      Complete pre-built v0 transaction.
                    //   2. Jupiter swap-instructions (/v6/quote + /v6/swap-instructions)
                    //      Individual instructions assembled into a real v0 transaction
                    //      with embedded Jito tip (satisfies Jito tip account write-lock).
                    //   3. Solend flash loan (atomic borrow+repay wrapper — last resort).

                    let operator_pubkey: String = flash_keypair
                        .as_ref()
                        .map(|kp| kp.pubkey_b58.clone())
                        .unwrap_or_default();

                    // Extract the swap direction from the arb path:
                    // first_from = input token, last_to = output token (for the main leg)
                    let ultra_input_mint: Option<String> = trade
                        .path.edges.first()
                        .map(|e| bs58::encode(e.from.0).into_string());
                    let ultra_output_mint: Option<String> = trade
                        .path.edges.get(1)
                        .map(|e| bs58::encode(e.to.0).into_string())
                        .or_else(|| trade.path.edges.last()
                            .map(|e| bs58::encode(e.to.0).into_string()));

                    let ultra_result = if !operator_pubkey.is_empty() {
                        if let (Some(ref input_mint), Some(ref output_mint)) =
                            (&ultra_input_mint, &ultra_output_mint)
                        {
                            if input_mint != output_mint {
                                let active_slippage = self_optimizer.params().slippage_bps;
                                match get_best_route_and_transaction(
                                    &ultra_client,
                                    input_mint,
                                    output_mint,
                                    trade.position_lamports,
                                    &operator_pubkey,
                                    config.jupiter_api_key.as_deref(),
                                    active_slippage,
                                )
                                .await
                                {
                                    Ok(route_data) => {
                                        // Guard: never use an empty transaction — fall back
                                        if route_data.transaction_bytes.is_empty() {
                                            warn!(
                                                input_mint  = %input_mint,
                                                output_mint = %output_mint,
                                                "Ultra API returned zero-length transaction bytes — falling back to swap-instructions"
                                            );
                                            None
                                        } else {
                                            info!(
                                                input_mint   = %input_mint,
                                                output_mint  = %output_mint,
                                                in_amount    = route_data.in_amount,
                                                out_amount   = route_data.out_amount,
                                                impact_pct   = format!("{:.4}%", route_data.price_impact_pct),
                                                tx_bytes     = route_data.transaction_bytes.len(),
                                                // route_hops = route_data.route_plan.len(), // removed because field doesn't exist in this commit
                                                request_id   = %route_data.request_id,
                                                "Ultra API: real swap transaction fetched — submitting to Jito"
                                            );
                                            Some(route_data.transaction_bytes)
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            error       = %e,
                                            input_mint  = %input_mint,
                                            output_mint = %output_mint,
                                            "Ultra API failed — falling back to flash loan path"
                                        );
                                        None
                                    }
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    // ── Compute tip for embedded Jito tip instruction ──────
                    let swap_tip_lamports = tip_calculator
                        .compute_tip(sim_result.expected_profit_lamports)
                        .unwrap_or(10_000);
                    let jito_tip_account = select_random_tip_account();

                    // ── Build execution path ───────────────────────────────
                    //
                    // Priority chain for transaction construction:
                    //   1. Jupiter Ultra API (/ultra/v1/order)
                    //      Returns a complete pre-built v0 transaction.
                    //   2. Jupiter swap-instructions (/v6/quote + /v6/swap-instructions)
                    //      Returns individual instructions with full account metas;
                    //      we assemble a real v0 transaction with embedded Jito tip.
                    //   3. Solend flash loan (last resort — wraps whatever swap data
                    //      is available in an atomic borrow+repay transaction).
                    //   If none succeed → skip this trade (never send invalid bytes).

                    let payloads: Vec<Vec<u8>> = if let Some(ultra_tx) = ultra_result {
                        // ── PATH 1: Jupiter Ultra — complete pre-built transaction
                        vec![ultra_tx]
                    } else {
                        // ── PATH 2: Jupiter swap-instructions — real v0 transaction
                        let swap_builder_result = if let Some(ref fkp) = flash_keypair {
                            if let (Some(ref input_mint), Some(ref output_mint)) =
                                (&ultra_input_mint, &ultra_output_mint)
                            {
                                if input_mint != output_mint {
                                    let active_slippage = self_optimizer.params().slippage_bps;
                                    let kp_arc = fkp.clone();
                                    match build_signed_swap_transaction(
                                        &swap_client,
                                        input_mint,
                                        output_mint,
                                        trade.position_lamports,
                                        &operator_pubkey,
                                        &fkp.pubkey_bytes,
                                        config.jupiter_api_key.as_deref(),
                                        active_slippage,
                                        swap_tip_lamports,
                                        &jito_tip_account,
                                        &|msg: &[u8]| kp_arc.sign(msg),
                                    )
                                    .await
                                    {
                                        Ok(built) => {
                                            info!(
                                                tx_bytes     = built.transaction_bytes.len(),
                                                in_amount    = built.in_amount,
                                                out_amount   = built.out_amount,
                                                tip_lamports = built.jito_tip_lamports,
                                                tip_account  = %jito_tip_account,
                                                "Jupiter swap-instructions: real v0 tx built — submitting to Jito"
                                            );
                                            Some(built.transaction_bytes)
                                        }
                                        Err(e) => {
                                            warn!(
                                                error = %e,
                                                "Jupiter swap-instructions failed — trying flash loan fallback"
                                            );
                                            None
                                        }
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        if let Some(swap_tx) = swap_builder_result {
                            vec![swap_tx]
                        } else if config.flash_loan_enabled {
                            // ── PATH 3: Solend flash loan (last-resort atomic wrap) ──
                            if let (Some(ref plan), Some(ref fkp)) =
                                (&flash_plan_for_live, &flash_keypair)
                            {
                                match SolanaRpcClient::new(&config.http_rpc_url).ok() {
                                    Some(rpc) => match rpc.get_latest_blockhash().await {
                                        Ok(bh_info) => {
                                            let swap_data: Vec<(String, Vec<u8>)> = trade
                                                .instructions
                                                .iter()
                                                .zip(trade.path.edges.iter())
                                                .map(|(instr, edge)| {
                                                    let prog = match edge.dex {
                                                        common::types::Dex::Raydium => "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
                                                        common::types::Dex::Orca    => "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3sFjJ37",
                                                        common::types::Dex::Meteora => "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",
                                                        common::types::Dex::Phoenix => "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY",
                                                        _                           => "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
                                                    };
                                                    (prog.to_string(), instr.data.clone())
                                                })
                                                .collect();

                                                let flash_tx = build_flash_loan_tx(
                                                    fkp,
                                                    &bh_info.blockhash,
                                                    plan.borrow_amount,
                                                    plan.repay_amount,
                                                    &trade.instructions.iter().map(|instr| {
                                                        // Convert your internal instruction to TxInstruction
                                                        ( "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".to_string(), instr.data.clone() )
                                                    }).collect::<Vec<_>>(),
                                                );

                                            info!(
                                                tx_bytes     = flash_tx.len(),
                                                borrow_sol   = format!("{:.6}", plan.borrow_amount as f64 / 1e9),
                                                repay_sol    = format!("{:.6}", plan.repay_amount as f64 / 1e9),
                                                fee_lamports = plan.fee_lamports,
                                                blockhash    = %bh_info.blockhash,
                                                "FLASH LOAN TX (last-resort): atomic borrow+repay → Jito"
                                            );
                                            vec![flash_tx]
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Flash loan: blockhash fetch failed — skipping trade");
                                            continue;
                                        }
                                    },
                                    None => {
                                        warn!("Flash loan: RPC init failed — skipping trade");
                                        continue;
                                    }
                                }
                            } else {
                                warn!("Flash loan: no viable plan or keypair — skipping trade");
                                continue;
                            }
                        } else {
                            warn!(
                                iteration,
                                "No valid execution path (Ultra + swap-instructions both failed, flash loans disabled) — skipping trade"
                            );
                            continue;
                        }
                    };

                    match jito.submit(payloads, sim_result.expected_profit_lamports).await {
                        Ok(bundle) => {
                            let profit = sim_result.expected_profit_lamports as i64;
                            pnl.add(profit);
                            session_stats.record_trade(profit);
                            metrics.bundles_submitted.inc();
                            metrics.total_profit_lamports.add(profit as f64);
                            // Only count accepted bundles against the circuit breaker
                            circuit_breaker.record_trade(profit).ok();
                            self_optimizer.record_trade(profit);

                            // Update rate limiter and dedup state
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
                            // Bundle was NOT accepted by Jito (429, signing failure,
                            // network error, etc.) — this is NOT a real trading loss.
                            // Do NOT update circuit breaker, PnL, or session stats.
                            // Only inform the optimizer so it can adapt parameters.
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
