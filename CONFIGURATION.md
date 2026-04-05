# Apex-MEV Neural Core 3.0 — Configuration Guide

## Quick Start

```bash
# Copy and edit the example env file
cp .env.example .env
```

The bot runs in **simulation mode** by default (`APEX_SIMULATION_ONLY=true`).
No real trades are sent. No wallet or real API key is required for the demo.

---

## Environment Variables

### Required

| Variable | Example | Description |
|---|---|---|
| `APEX_RPC_URL` | `wss://mainnet.helius-rpc.com/?api-key=...` | Solana WebSocket RPC |
| `APEX_HTTP_RPC_URL` | `https://api.mainnet-beta.solana.com` | Solana HTTP RPC (for getSlot, getBalance) |
| `APEX_KEYPAIR_PATH` | `/tmp/mock-keypair.json` | Path to Solana keypair JSON (64-byte array) |
| `APEX_JITO_URL` | `https://mainnet.block-engine.jito.wtf` | Jito block engine endpoint |

### Optional — Price Data

| Variable | Default | Description |
|---|---|---|
| `HELIUS_API_KEY` | _(none)_ | Enables real-time 12-token price feeds via Helius DAS API. Without this, the bot uses synthetic mock data. |

### Optional — Strategy

| Variable | Default | Description |
|---|---|---|
| `APEX_MIN_PROFIT_LAMPORTS` | `10000` | Minimum net profit to attempt a trade (lamports). 10 000 = 0.00001 SOL |
| `APEX_MAX_HOPS` | `4` | Maximum edges per arbitrage path (2–6). Higher = more paths found, longer evaluation. |
| `APEX_MAX_POSITION_LAMPORTS` | `1000000000` | Maximum capital deployed per trade (lamports). 1e9 = 1 SOL |
| `APEX_SLIPPAGE_BPS` | `50` | Intermediate-hop slippage tolerance in basis points. 50 = 0.5% |

### Optional — Flash Loans

| Variable | Default | Description |
|---|---|---|
| `APEX_FLASH_LOAN_ENABLED` | `false` | Use Solend flash loans (borrow + repay in one atomic tx). Enables capital-free arb. Fee: 9 bps per loan. |

### Optional — Self-Optimizer

| Variable | Default | Description |
|---|---|---|
| `APEX_AUTO_OPTIMIZE` | `true` | Automatically adjust `slippage_bps`, `min_profit`, and Jito tip fraction every 50 trades. |

### Optional — Risk Management

| Variable | Default | Description |
|---|---|---|
| `APEX_CB_THRESHOLD_LAMPORTS` | `5000000000` | Circuit breaker trips after cumulative losses exceed this value (5 SOL default). |
| `APEX_CB_CONSECUTIVE_LOSSES` | `5` | Circuit breaker trips after N consecutive losing trades. |
| `APEX_MAX_DAILY_LOSS_LAMPORTS` | `500000000` | Bot halts if total session losses exceed 0.5 SOL. |

### Optional — Simulation / Mode

| Variable | Default | Description |
|---|---|---|
| `APEX_SIMULATION_ONLY` | `true` | When `true`, bundles are logged but **never submitted**. Safe for testing. |
| `RUST_LOG` | `info` | Log verbosity. Use `debug` or `trace` for hot-path diagnostics. |

---

## Operating Modes

### 1. Full Simulation (default — no API keys needed)

```dotenv
APEX_SIMULATION_ONLY=true
APEX_RPC_URL=wss://api.mainnet-beta.solana.com
APEX_HTTP_RPC_URL=https://api.mainnet-beta.solana.com
APEX_KEYPAIR_PATH=/tmp/mock-keypair.json
APEX_JITO_URL=https://mainnet.block-engine.jito.wtf
```

**What you get:** MockShredStream (400 synthetic shreds/sec), 12-token mock price matrix, full RICH Bellman-Ford + GNN pipeline, self-optimizer cycling, P&L tracking — all without spending any SOL.

---

### 2. Simulation with Real Prices (needs Helius key)

Add `HELIUS_API_KEY=your_key_here` to the above.

**What you get:** Everything in mode 1, **plus** real-time USD prices for SOL, USDC, USDT, RAY, ORCA, JUP, mSOL, JITO, BONK, WIF, PYTH, RENDER — polled every 1500ms. The RICH engine will now detect real market inefficiencies.

---

### 3. Live Trading (advanced)

```dotenv
APEX_SIMULATION_ONLY=false
HELIUS_API_KEY=your_key_here
APEX_KEYPAIR_PATH=/path/to/funded-keypair.json
```

**Requirements:**
- Funded Solana wallet (minimum 0.1 SOL for fees and tip)
- HELIUS_API_KEY for real transaction stream
- Production RPC endpoint (not public api.mainnet-beta.solana.com — too slow)

**Risk controls active in live mode:**
- Circuit breaker (trips after 5 consecutive losses or 5 SOL cumulative loss)
- Anomaly detector (skips trades with statistically unusual profit sizes)
- Atomic revert guard (rolls back position tracking on failed bundles)
- Pre-simulation (rejects trades where local sim shows insufficient profit)
- Flash loan viability check (skips if expected profit < Solend fee)

---

## Token Universe (12 tokens, Phase 3)

| Symbol | Protocol | Role |
|---|---|---|
| SOL | Native | Numeraire for most pairs |
| USDC | Centre | Primary stablecoin |
| USDT | Tether | Secondary stablecoin |
| RAY | Raydium | DEX governance |
| ORCA | Orca | DEX governance |
| JUP | Jupiter | Aggregator governance |
| mSOL | Marinade | Liquid staking |
| JITO | Jito | Liquid staking |
| BONK | Bonk | High-volume meme |
| WIF | Dogwifhat | High-volume meme |
| PYTH | Pyth Network | Oracle infra |
| RENDER | Render Network | Compute infra |

---

## Per-DEX Simulation Fees (Phase 5)

The pre-simulator uses realistic fee rates — not the old flat 0.3%:

| DEX | Fee | Notes |
|---|---|---|
| Raydium AMM v4 | 0.25% (25 bps) | Fixed pool fee |
| Orca Whirlpool | 0.30% (30 bps) | Default tier (varies 0.01%–2%) |
| Meteora DAMM | 0.20% (20 bps) | Dynamic fee (conservative estimate) |
| Phoenix CLOB | 0.10% (10 bps) | Taker fee |
| Jupiter V6 | 0.30% (30 bps) | Composite of routed pools |

---

## Self-Optimizer (Phase 10)

Every 50 evaluated trades, the optimizer adjusts three parameters:

| Parameter | Adjust Up When | Adjust Down When |
|---|---|---|
| `slippage_bps` | Sim acceptance rate < 40% | Accept rate > 85% AND win rate > 75% |
| `min_profit_lamports` | Win rate < 30% or avg profit < 2× min | Avg profit > 10× min |
| `tip_fraction_pct` | Win rate < 40% (need block space) | Win rate > 80% (over-paying) |

All parameters are bounded by hard safety floors/ceilings. Every change is logged.

---

## Flash Loans (Phase 7, Solend)

When `APEX_FLASH_LOAN_ENABLED=true`, each approved trade wraps its swap sequence
with Solend borrow + repay instructions in a single atomic transaction:

```
[0] FlashBorrowReserveLiquidity  ← borrow capital from Solend SOL reserve
[1..N] ArbitrageSwaps            ← execute arb path with borrowed capital
[N+1] FlashRepayReserveLiquidity ← repay principal + 0.09% fee
```

If the arb doesn't generate enough profit to repay, the Solana runtime reverts
the entire transaction atomically — no funds are lost.

---

## Generating a Keypair

```bash
# Using Solana CLI
solana-keygen new -o /path/to/keypair.json

# Or use the mock keypair (simulation mode only)
python3 -c "import json,os; print(json.dumps(list(os.urandom(64))))" > /tmp/mock-keypair.json
```

---

## Prometheus Metrics

The bot exposes the following metrics:

| Metric | Description |
|---|---|
| `apex_paths_evaluated_total` | Total RICH cycles run |
| `apex_paths_profitable_total` | Paths approved by strategy |
| `apex_bundles_submitted_total` | Jito bundles submitted (live mode) |
| `apex_total_profit_lamports` | Cumulative P&L (lamports) |
| `apex_hot_path_duration_seconds` | Hot loop latency histogram |
| `apex_circuit_breaker_state` | 1 = closed (trading), 0 = open (halted) |
