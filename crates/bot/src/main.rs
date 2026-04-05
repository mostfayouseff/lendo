// =============================================================================
// APEX-MEV Neural Core 3.0 — LIVE TRADING MODE
//
// INGRESS PRIORITY:
//   1. Helius WebSocket (PRIMARY)   — wss://mainnet.helius-rpc.com/?api-key=<KEY>
//   2. Alchemy WebSocket (FALLBACK) — wss://solana-mainnet.g.alchemy.com/v2/<KEY>
//   3. Mock stream (LAST RESORT)    — logged clearly, indicates no WS keys set
//
// PRICE DATA (self-healing):
//   1. https://api.jup.ag/price/v2  — newest endpoint
//   2. https://price.jup.ag/v6/price — legacy fallback
//   3. Stale cache                   — last resort (logged)
//
// LIVE EXECUTION:
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
use ingress::{HeliusTransactionStream, AlchemyTransactionStream, JupiterMonitor, MockShredStream, MockYellowstoneStream};
use jito_handler::{build_flash_loan_tx, ApexKeypair, JitoBundleHandler, SolanaRpcClient};
use pnl::{make_record, AtomicPnL, SessionStats};
use risk_oracle::{AnomalyDetector, CircuitBreaker, SelfOptimizer, TradingParams};
use safety::{AtomicRevertGuard, PreSimulator};
use solana_program_apex::instruction::dex_fee_bps;
use std::sync::Arc;
use strategy::{ArbitrageStrategy, SolendFlashLoan};
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

            for trade in approved_trades {
                let dex_path: String = trade
                    .path
                    .edges
                    .iter()
                    .map(|e| format!("{}", e.dex))
                    .collect::<Vec<_>>()
                    .join(" → ");

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
                    "Arbitrage opportunity detected"
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
                    let payloads: Vec<Vec<u8>> = if config.flash_loan_enabled {
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
                                                    common::types::Dex::Raydium  => "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
                                                    common::types::Dex::Orca     => "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3sFjJ37",
                                                    common::types::Dex::Meteora  => "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",
                                                    common::types::Dex::Phoenix  => "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY",
                                                    _                            => "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
                                                };
                                                (prog.to_string(), instr.data.clone())
                                            })
                                            .collect();

                                        let flash_tx = build_flash_loan_tx(
                                            fkp,
                                            &bh_info.blockhash,
                                            plan.borrow_amount,
                                            plan.repay_amount,
                                            &swap_data,
                                        );

                                        info!(
                                            tx_bytes     = flash_tx.len(),
                                            borrow_sol   = format!("{:.6}", plan.borrow_amount as f64 / 1e9),
                                            repay_sol    = format!("{:.6}", plan.repay_amount as f64 / 1e9),
                                            fee_lamports = plan.fee_lamports,
                                            blockhash    = %bh_info.blockhash,
                                            "LIVE FLASH LOAN TX: atomic borrow+swap+repay — submitting to Jito"
                                        );

                                        vec![flash_tx]
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "Flash loan: blockhash fetch failed — using swap-only fallback");
                                        trade.instructions.iter().map(|i| i.data.clone()).collect()
                                    }
                                },
                                None => {
                                    warn!("Flash loan: RPC init failed — using swap-only fallback");
                                    trade.instructions.iter().map(|i| i.data.clone()).collect()
                                }
                            }
                        } else {
                            warn!("Flash loan: no viable plan or keypair — using swap-only path");
                            trade.instructions.iter().map(|i| i.data.clone()).collect()
                        }
                    } else {
                        trade.instructions.iter().map(|i| i.data.clone()).collect()
                    };

                    match jito.submit(payloads, sim_result.expected_profit_lamports).await {
                        Ok(bundle) => {
                            let profit = sim_result.expected_profit_lamports as i64;
                            pnl.add(profit);
                            session_stats.record_trade(profit);
                            metrics.bundles_submitted.inc();
                            metrics.total_profit_lamports.add(profit as f64);
                            circuit_breaker.record_trade(profit).ok();
                            self_optimizer.record_trade(profit);

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
                            error!(error = %e, iter = iteration, "Bundle submission failed — retrying next cycle");
                            circuit_breaker.record_trade(-1000).ok();
                            pnl.add(-1000);
                            session_stats.record_trade(-1000);
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
