# Apex-MEV Neural Core 3.0

A high-performance, production-grade Solana MEV (Maximal Extractable Value) arbitrage system built in Rust. Fully live on Solana Mainnet — no simulation, no mocks in the hot path.

## Architecture

Rust workspace with nine specialized crates:

- **`crates/bot`** — Main entry point; wires all components. Hot loop includes RICH engine, self-optimizer, flash loan planner, per-DEX simulation, latency tracking.
- **`crates/core`** (apex-core) — RICH Bellman-Ford negative-cycle detection (AVX2 SIMD) + GNN confidence scoring. f64-native PriceMatrix; FxHashMap (ahash) for token index lookups.
- **`crates/ingress`** — Helius WebSocket (PRIMARY, transactionSubscribe + logsSubscribe), Alchemy WebSocket (FALLBACK, logsSubscribe), self-healing Jupiter price monitor (3-endpoint fallback chain), mock streams.
- **`crates/strategy`** — Arbitrage pipeline, multi-DEX routing (Raydium/Orca/Meteora/Phoenix/Jupiter), Solend flash loans, flash swap instruction builder.
- **`crates/jito-handler`** — Jito bundle construction, dynamic tip calculation, live bundle submission.
- **`crates/risk-oracle`** — Circuit Breaker, Anomaly Detector, SelfOptimizer (auto-adjusts slippage/min_profit/tip every 50 trades).
- **`crates/safety`** — PreSimulator (per-DEX fee rates), Atomic Revert Guards.
- **`crates/common`** — Shared types (PriceMatrix: f64-native), Prometheus metrics, configuration.
- **`crates/solana-program`** — On-chain instruction codecs, per-DEX fee simulation, profit guard.

## Live Trading Architecture (Phase 14+)

### Ingress Priority Chain
1. **Helius WebSocket (PRIMARY)** — `wss://mainnet.helius-rpc.com/?api-key=<KEY>`  
   Uses both `logsSubscribe` (standard Solana) and `transactionSubscribe` (Helius-enhanced) on 7 DEX programs. Full self-healing reconnect with exponential back-off.
2. **Alchemy WebSocket (FALLBACK)** — `wss://solana-mainnet.g.alchemy.com/v2/<KEY>`  
   Standard `logsSubscribe` on same 7 DEX programs. Activates only if Helius key missing.
3. **MockShredStream (LAST RESORT)** — Only used if neither Helius nor Alchemy keys are set. Logged clearly.

### Price Feed Priority Chain (Self-Healing)
1. `https://api.jup.ag/price/v2` — newest Jupiter endpoint
2. `https://price.jup.ag/v6/price` — legacy Jupiter endpoint
3. `https://lite-api.jup.ag/price/v2` — lite fallback
4. Stale cache — last resort, always logged clearly

### Flash Loans
- Solend atomic borrow + multi-DEX swap + repay in a single Solana transaction
- No pre-funded balance required — capital borrowed atomically per trade
- Submitted via Jito bundle for MEV protection and tip-based priority

### No Restrictions
- `APEX_MIN_PROFIT_LAMPORTS=0` — all arbitrage paths attempted, no minimum profit gate
- No minimum balance requirement — flash loans supply the capital

## Configuration

All environment variables are set in Replit Secrets/env vars.

| Variable | Value | Description |
|---|---|---|
| `APEX_SIMULATION_ONLY` | `false` | Live trading (NOT simulation) |
| `HELIUS_API_KEY` | set | PRIMARY WebSocket stream key |
| `ALCHEMY_API_KEY` | set | FALLBACK WebSocket stream key |
| `JUPITER_API_KEY` | set | Authenticated Jupiter price endpoint |
| `APEX_FLASH_LOAN_ENABLED` | `true` | Solend atomic borrow+repay |
| `APEX_AUTO_OPTIMIZE` | `true` | Self-optimizer active |
| `APEX_MIN_PROFIT_LAMPORTS` | `0` | No minimum profit threshold |
| `APEX_MAX_HOPS` | `4` | Max arb path length |
| `APEX_SLIPPAGE_BPS` | `50` | Intermediate hop slippage |
| `APEX_KEYPAIR_PATH` | `live-key.json` | Operator keypair path |
| `RUST_LOG` | `info` | Log verbosity |

## Keypair

Operator keypair is at `live-key.json`.  
Public key: `9yNWW6SrmoG8tWyWW2KGmpk9ge4VfF4zSjDkv4jy4PtN`  
Format: Solana JSON keypair [64 bytes — ed25519 private (32) + public (32)]

## DEX Programs Monitored

- Raydium AMM v4: `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8`
- Raydium CLMM: `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK`
- Orca Whirlpools: `whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3sFjJ37`
- Meteora DLMM: `LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo`
- Meteora Dynamic AMM: `Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB`
- Phoenix DEX: `PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY`
- Jupiter V6: `JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4`

## Token Universe (12 tokens)

SOL, USDC, USDT, RAY, ORCA, JUP, mSOL, JitoSOL, BONK, WIF, PYTH, RENDER  
→ 132 directed edges across 5 DEX venues

## Per-DEX Simulation Fees

Raydium 25bps | Orca 30bps | Meteora 20bps | Phoenix 10bps | Jupiter 30bps

## Key Dependencies

- `tokio` — Async multi-threaded runtime
- `rustc-hash` — ahash-backed FxHashMap for O(1) token index lookups
- `rust_decimal` — High-precision decimal math for Decimal fields (MarketEdge)
- `ed25519-dalek` — Solana transaction signing
- `reqwest` + `tokio-tungstenite` — HTTP/WebSocket for Jupiter Price API, Helius, Alchemy, Jito
- `curve25519-dalek` — Ed25519 on-curve check for Solend PDA derivation
- `prometheus` — Real-time metrics
- `serde` + `serde_json` — Serialization
- `bs58` — Base58 encoding/decoding for Solana pubkeys
