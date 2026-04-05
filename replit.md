# Apex-MEV Neural Core 3.0

A high-performance, production-grade Solana MEV (Maximal Extractable Value) arbitrage prototype built in Rust, optimized across 13 phases.

## Architecture

Rust workspace with nine specialized crates:

- **`crates/bot`** — Main entry point; wires all components. Hot loop includes RICH engine, self-optimizer, flash loan planner, per-DEX simulation, latency tracking.
- **`crates/core`** (apex-core) — RICH Bellman-Ford negative-cycle detection (AVX2 SIMD) + GNN confidence scoring. f64-native PriceMatrix; FxHashMap (ahash) for token index lookups.
- **`crates/ingress`** — Jupiter Price API v6 monitor (12 tokens, 1500ms poll, no API key), Alchemy WebSocket (logsSubscribe, standard Solana WS), mock streams.
- **`crates/strategy`** — Arbitrage pipeline, multi-DEX routing (Raydium/Orca/Meteora/Phoenix/Jupiter), Solend flash loans, flash swap instruction builder.
- **`crates/jito-handler`** — Jito bundle construction, dynamic tip calculation, live bundle submission.
- **`crates/risk-oracle`** — Circuit Breaker, Anomaly Detector, SelfOptimizer (auto-adjusts slippage/min_profit/tip every 50 trades).
- **`crates/safety`** — PreSimulator (per-DEX fee rates, Phase 5), Atomic Revert Guards.
- **`crates/common`** — Shared types (PriceMatrix: f64-native), Prometheus metrics, configuration.
- **`crates/solana-program`** — On-chain instruction codecs, per-DEX fee simulation, profit guard.

## 13-Phase Optimization Summary

| Phase | Change |
|---|---|
| 1-2 | `rustc-hash` (ahash FxHashMap), f64-native PriceMatrix — eliminates 400 Decimal→f64 conversions |
| 3-4 | 12-token universe (was 3), 1500ms poll (was 3500ms), in-memory price cache |
| 5-6 | Per-DEX AMM fees in pre-simulator; route cache foundation |
| 7 | SolendFlashLoan wired into hot loop (plan + viability check + sim logging) |
| 8-9 | `tokio::task::spawn_blocking` for strategy evaluation; stage latency in logs |
| 10 | SelfOptimizer wired: records every sim/trade result; cycles every 50 trades |
| 11-12 | 12-token mock edge set; synthetic triangular arb cycle for cycle detection testing |
| 13 | CONFIGURATION.md with all env vars, modes, per-DEX fees, flash loan usage |

## Running

The project runs via the "Start application" workflow:
```
cargo run --bin apex-mev
```

Default: simulation mode (`APEX_SIMULATION_ONLY=true`) — no real trades.

## Configuration

See `CONFIGURATION.md` for the full reference. Key variables:

| Variable | Default | Description |
|---|---|---|
| `APEX_SIMULATION_ONLY` | `true` | `false` = live trading |
| `ALCHEMY_API_KEY` | (none) | Enables real on-chain DEX log stream (Alchemy WS) |
| `APEX_FLASH_LOAN_ENABLED` | `false` | Solend atomic borrow+repay |
| `APEX_AUTO_OPTIMIZE` | `true` | Self-optimizer active |
| `APEX_MIN_PROFIT_LAMPORTS` | `10000` | Min profit threshold |
| `APEX_MAX_HOPS` | `4` | Max arb path length |
| `APEX_SLIPPAGE_BPS` | `50` | Intermediate hop slippage |
| `RUST_LOG` | `info` | Log verbosity |

## Ingress Modes

- **With `ALCHEMY_API_KEY`**: Alchemy WebSocket (`logsSubscribe` on 7 DEX programs, real mainnet txs) + Jupiter Price API v6 (no key, public)
- **Without `ALCHEMY_API_KEY`**: MockShredStream (400 shreds/sec) + 12-token mock matrix

## Flash Loan (Solend)

- Enabled via `APEX_FLASH_LOAN_ENABLED=true` + live mode + `APEX_FLASH_KEYPAIR_PATH=<path>` (optional, falls back to main keypair)
- `flash_tx.rs` builds an atomic Solana legacy tx: borrow (disc 0x14) + swap stubs + repay (disc 0x15)
- PDA (lending market authority) derived via sha256 + curve25519 off-curve check; ATA derived per SPL spec
- Swap instruction bodies are stubs — for production wire in Jupiter `/v6/swap-instructions`
- Mainnet Solend accounts: Program `So1endDq...`, Pool `4UpD2f...`, Reserve `FzbfXR...`
- 6 unit tests cover: base58 decode, ATA determinism, borrow/repay discriminators, compact-u16, PDA off-curve

## Token Universe (12 tokens)

SOL, USDC, USDT, RAY, ORCA, JUP, mSOL, JITO, BONK, WIF, PYTH, RENDER  
→ 132 directed edges (was 6 with 3 tokens)  
→ Synthetic SOL→USDC→RAY→SOL triangle (-0.19 log weight) for cycle detection demo

## Per-DEX Simulation Fees

Raydium 25bps | Orca 30bps | Meteora 20bps | Phoenix 10bps | Jupiter 30bps

## Key Dependencies

- `tokio` — Async multi-threaded runtime
- `rustc-hash` — ahash-backed FxHashMap for O(1) token index lookups
- `rust_decimal` — High-precision decimal math for Decimal fields (MarketEdge)
- `ed25519-dalek` — Solana transaction signing
- `reqwest` + `tokio-tungstenite` — HTTP/WebSocket for Jupiter Price API, Alchemy, Jito
- `curve25519-dalek` — Ed25519 on-curve check for Solend PDA derivation
- `prometheus` — Real-time metrics
- `serde` + `serde_json` — Serialization
