# Apex MEV Platform

A production-grade Solana Flash Loan Arbitrage Platform built on top of the Apex-MEV Neural Core 3.0 Rust engine. Includes a full REST+WebSocket API server, PostgreSQL/Redis-backed storage, JWT/RBAC authentication, Anchor smart contract for on-chain atomic flash loan arbitrage, HTML+TailwindCSS real-time dashboard, Prometheus/Grafana monitoring, and Docker deployment.

## Architecture

Rust workspace with specialized crates:

- **`crates/bot`** — MEV engine entry point; validates live config, ingests price data, evaluates Bellman-Ford arbitrage paths, requests Jupiter Ultra orders, simulates, attaches Jito tip, signs, sends, and confirms.
- **`crates/api`** — Axum HTTP + WebSocket API server; all REST endpoints, JWT middleware, Prometheus metrics, static frontend serving.
- **`crates/auth`** — JWT generation/validation, bcrypt password hashing, RBAC middleware, refresh-token session management.
- **`crates/cache`** — Redis client wrapper, session store, sliding-window rate limiter.
- **`crates/db`** — SQLx PostgreSQL models (13 migrations), repository pattern for all 11 entities.
- **`crates/flash-loans`** — Solend, MarginFi, Kamino provider adapters with atomic execution flow.
- **`crates/core`** — RICH Bellman-Ford negative-cycle detection and price matrix construction.
- **`crates/ingress`** — Helius WebSocket primary stream, Alchemy fallback, Jupiter price monitor, Ultra order client.
- **`crates/strategy`** — Arbitrage path evaluation and trade sizing.
- **`crates/jito-handler`** — Solana RPC helpers, keypair handling, transaction signing, Jito tip attachment.
- **`crates/risk-oracle`** — Circuit breaker, anomaly detector, self-optimizer.
- **`crates/safety`** — Pre-simulation helpers and atomic revert guard.
- **`crates/common`** — Shared types, metrics, configuration.
- **`crates/solana-program`** — On-chain instruction codecs and DEX fee simulation helpers.
- **`smart-contracts/`** — Anchor program `apex-arb` (separate compilation via `anchor build`).
- **`frontend/dist/`** — HTML+vanilla JS+TailwindCSS 10-page dashboard served as static files.

## Running

### Development — MEV bot only
```
cargo run --bin apex-mev
```

### Development — API server + dashboard
```
cargo run --bin apex-api
# Open http://localhost:8080 in browser
```

### Docker (full stack)
```
make docker-up
# API:     http://localhost:8080
# Grafana: http://localhost:3001
```

## Workflows

| Workflow | Command | Description |
|---|---|---|
| Start application | `cargo run --bin apex-mev` | Live MEV arbitrage engine |
| Start API server | `cargo run --bin apex-api` | REST API + WebSocket + Dashboard |

## Required Environment Variables

Copy `.env.example` to `.env` and fill in all values.

| Variable | Required for | Description |
|---|---|---|
| `DATABASE_URL` | API | PostgreSQL connection string |
| `REDIS_URL` | API | Redis connection string |
| `JWT_SECRET` | API | ≥64-char random secret for JWT signing |
| `ADMIN_EMAIL` | API | Initial admin account email |
| `ADMIN_PASSWORD` | API | Initial admin account password |
| `APEX_HTTP_RPC_URL` | Both | Solana HTTP RPC endpoint |
| `APEX_RPC_URL` | Both | Solana WebSocket RPC endpoint |
| `HELIUS_API_KEY` or `ALCHEMY_API_KEY` | MEV bot | Live price ingress source |
| `JUPITER_API_KEY` | MEV bot | Jupiter Ultra API |
| `JITO_TIP_ACCOUNT` | MEV bot | Jito tip destination pubkey |
| `APEX_SIMULATION_ONLY` | MEV bot | `false` for live execution |
| `APEX_KEYPAIR_PATH` | MEV bot | Operator keypair JSON path |
| `RUST_LOG` | Both | Log verbosity (`info`, `debug`) |

## API Endpoints

All endpoints are under `/api/v1/`. Authentication via `Authorization: Bearer <token>`.

| Method | Path | Description |
|---|---|---|
| POST | `/auth/login` | Login → JWT tokens |
| POST | `/auth/refresh` | Refresh access token |
| POST | `/auth/logout` | Revoke session |
| GET | `/users/me` | Current user profile |
| GET/POST | `/wallets` | List / add operator wallets |
| GET/POST | `/strategies` | List / create strategies |
| GET | `/trades` | Trade history |
| GET | `/trades/summary` | Win rate, P&L summary |
| GET/POST | `/opportunities` | Detected arb opportunities |
| GET | `/flash-loans/providers` | Available flash loan providers |
| POST | `/flash-loans/quote` | Get flash loan fee quote |
| GET/POST | `/settings` | System configuration |
| GET/POST | `/risk/rules` | Risk management rules |
| POST | `/bot/start` | Start MEV bot |
| POST | `/bot/stop` | Stop MEV bot |
| GET | `/bot/status` | Bot running state |
| GET | `/monitoring/overview` | Dashboard overview metrics |
| GET | `/monitoring/system-events` | System event log |
| GET | `/health` | Health check |
| WS | `/ws` | Real-time WebSocket feed |

## Database Migrations

13 migrations in `database/migrations/`. Apply with:
```
psql $DATABASE_URL -f database/migrations/001_create_users.sql
# ... through 013_ip_address_to_text.sql
```
Or via Docker: migrations run automatically on `docker-compose up`.

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

## User Preferences

- No mocks, no placeholders, no TODOs — all code must be production-ready.
- Rust backend (Axum, SQLx, PostgreSQL, Redis).
- Frontend served as static HTML+TailwindCSS from the API server.
- Live execution uses Jupiter Ultra orders only; legacy `/swap/v2` is removed.
- Smart contracts compiled separately via `anchor build` (not part of Rust workspace).
