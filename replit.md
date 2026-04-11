# Apex-MEV Neural Core 3.0

A production-focused Solana MEV arbitrage system built in Rust. Live execution is configured to use Jupiter Ultra orders only for trade construction; mock execution and legacy Jupiter Swap V2 trade endpoints are disabled.

## Architecture

Rust workspace with specialized crates:

- **`crates/bot`** — Main entry point; validates live configuration, runs ingress, evaluates opportunities, requests Jupiter Ultra orders, simulates, attaches Jito tip, signs, sends, and confirms transactions.
- **`crates/core`** (apex-core) — RICH Bellman-Ford negative-cycle detection and price matrix construction.
- **`crates/ingress`** — Helius WebSocket primary stream, Alchemy fallback stream, Jupiter price monitor, and Jupiter Ultra order client.
- **`crates/strategy`** — Arbitrage path evaluation and trade sizing.
- **`crates/jito-handler`** — Solana RPC helpers, keypair handling, transaction mutation/signing, and Jito tip attachment.
- **`crates/risk-oracle`** — Circuit breaker, anomaly detector, and self-optimizer.
- **`crates/safety`** — Pre-simulation helpers and atomic revert guard.
- **`crates/common`** — Shared types, metrics, and configuration.
- **`crates/solana-program`** — On-chain instruction codecs and DEX fee simulation helpers.

## Live Ultra Execution Pipeline

Every live trade follows this sequence:

1. Validate required environment and wallet safety gates.
2. Receive live ingress from Helius or Alchemy; mock ingress is rejected.
3. Evaluate candidate arbitrage paths from live price data.
4. Request a Jupiter Ultra order from `https://api.jup.ag/ultra/v1/order`.
5. Simulate the returned unsigned Ultra transaction.
6. Append the configured Jito System Program tip instruction.
7. Sign with the operator keypair.
8. Send the signed transaction through Solana RPC.
9. Confirm the signature on-chain.

Legacy Jupiter `/swap/v2/*` trade construction files have been removed from the ingress crate.

## Startup Execution Proof

- The bot can send a real Memo-program startup proof transaction when enabled.
- The proof transaction builds, simulates, signs, sends, and confirms independently of arbitrage profitability.
- Memo is used instead of a System Program transfer so startup proof works with non-empty source accounts.

## Ingress Priority Chain

1. **Helius WebSocket (PRIMARY)** — `wss://mainnet.helius-rpc.com/?api-key=<KEY>`
2. **Alchemy WebSocket (FALLBACK)** — `wss://solana-mainnet.g.alchemy.com/v2/<KEY>`

If neither key is configured, the bot exits. Mock ingress is not allowed for live execution.

## Required Configuration

All environment variables are set in Replit Secrets/env vars.

| Variable | Description |
|---|---|
| `APEX_SIMULATION_ONLY=false` | Required for live execution; simulation-only mode is rejected |
| `JUPITER_API_KEY` | Required for Jupiter Ultra API |
| `HELIUS_API_KEY` or `ALCHEMY_API_KEY` | Required live ingress source |
| `JITO_TIP_ACCOUNT` | Required Jito tip account pubkey |
| `JITO_TIP_LAMPORTS` | Jito tip amount; defaults to `1000` |
| `APEX_KEYPAIR_PATH` | Operator keypair path; defaults to `live-key.json` |
| `APEX_HTTP_RPC_URL` | Solana HTTP RPC URL |
| `APEX_RPC_URL` | Solana WebSocket RPC URL |
| `APEX_MIN_PROFIT_LAMPORTS` | Minimum opportunity threshold |
| `APEX_MAX_HOPS` | Maximum arbitrage path length |
| `APEX_MAX_POSITION_LAMPORTS` | Maximum trade input size |
| `RUST_LOG` | Log verbosity |

## Operator Wallet Safety

The configured operator wallet must be system-owned and carry no account data for the appended Jito System Program tip transfer. The bot validates owner/data length and requires enough SOL for Ultra fees plus the configured Jito tip.

## DEX Programs Monitored

- Raydium AMM v4: `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8`
- Raydium CLMM: `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK`
- Orca Whirlpools: `whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3sFjJ37`
- Meteora DLMM: `LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo`
- Meteora Dynamic AMM: `Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB`
- Phoenix DEX: `PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY`
- Jupiter V6: `JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4`

## Token Universe

SOL, USDC, USDT, RAY, ORCA, JUP, mSOL, JitoSOL, BONK, WIF, PYTH, RENDER
