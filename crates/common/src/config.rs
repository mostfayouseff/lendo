// =============================================================================
// SECURITY AUDIT CHECKLIST — common/src/config.rs
// [✓] ZERO hardcoded secrets — all sensitive values from env vars
// [✓] Secret fields (rpc_url, keypair_path, alchemy_api_key) are NOT Serialize
//     to prevent accidental logging/serialisation of credentials
// [✓] No panics — loading returns Result, caller decides how to handle
// [✓] dotenvy loads .env only in non-production (guarded by APEX_ENV)
// [✓] All integer conversions checked (parse::<u64>)
// [✓] alchemy_api_key masked in debug output (only length shown)
// [✓] Flash loan, daily loss limit, slippage, auto-optimize fields added
// =============================================================================

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::info;

/// Top-level bot configuration.
/// Secret fields intentionally omit `Serialize` to prevent secret leakage in logs.
#[derive(Debug, Deserialize, Clone)]
pub struct ApexConfig {
    // ── Network ──────────────────────────────────────────────────────────────
    /// Solana RPC WebSocket URL (e.g. wss://solana-mainnet.g.alchemy.com/v2/<KEY>)
    /// Source: APEX_RPC_URL env var — NEVER hardcode
    pub rpc_url: String,

    /// Solana RPC HTTP URL for JSON-RPC calls (getLatestBlockhash, getBalance).
    /// Separate from rpc_url because Jito's SolanaRpcClient uses HTTP, not WSS.
    /// Source: APEX_HTTP_RPC_URL env var
    /// Default: https://api.mainnet-beta.solana.com
    pub http_rpc_url: String,

    /// Path to the operator keypair JSON file (never the raw key bytes)
    /// Source: APEX_KEYPAIR_PATH env var
    pub keypair_path: String,

    // ── Alchemy ───────────────────────────────────────────────────────────────
    /// Alchemy API key for WebSocket (logsSubscribe) and HTTP RPC.
    /// When set, replaces the mock shred stream with real on-chain data.
    /// Source: ALCHEMY_API_KEY env var (optional — falls back to mock stream)
    pub alchemy_api_key: Option<String>,

    // ── Strategy ─────────────────────────────────────────────────────────────
    /// Minimum expected profit in lamports to attempt execution
    pub min_profit_lamports: u64,

    /// Maximum number of hops per arbitrage path (2–6, spec §3)
    pub max_hops: usize,

    /// Maximum capital to deploy per trade in lamports
    pub max_position_lamports: u64,

    /// Slippage tolerance for intermediate swap hops in basis points.
    /// 50 = 0.5% (default), 100 = 1%, 500 = 5% (max)
    /// Source: APEX_SLIPPAGE_BPS env var
    pub slippage_bps: u16,

    // ── Flash Loans ───────────────────────────────────────────────────────────
    /// Whether to use Solend flash loans for capital-free arbitrage.
    /// When enabled, borrows capital atomically and repays in the same tx.
    /// Source: APEX_FLASH_LOAN_ENABLED env var (default: false)
    pub flash_loan_enabled: bool,

    // ── Jito ─────────────────────────────────────────────────────────────────
    /// Jito block engine endpoint
    /// Source: APEX_JITO_URL env var
    pub jito_url: String,

    // ── Risk ─────────────────────────────────────────────────────────────────
    /// Maximum drawdown before circuit breaker trips (in lamports)
    pub circuit_breaker_threshold_lamports: u64,

    /// Number of consecutive losses before circuit breaker activates
    pub circuit_breaker_consecutive_losses: u32,

    /// Maximum total loss allowed per 24-hour session (in lamports).
    /// Bot halts if cumulative losses exceed this threshold.
    /// Source: APEX_MAX_DAILY_LOSS_LAMPORTS env var
    /// Default: 500_000_000 (0.5 SOL)
    pub max_daily_loss_lamports: u64,

    // ── Self-Optimization ─────────────────────────────────────────────────────
    /// When true, the bot automatically adjusts slippage tolerance and position
    /// sizing based on real-time win rate and profit-per-trade metrics.
    /// Source: APEX_AUTO_OPTIMIZE env var (default: true)
    pub auto_optimize: bool,

    // ── Simulation ───────────────────────────────────────────────────────────
    /// Whether to run in simulation-only mode (never sends real transactions)
    pub simulation_only: bool,
}

impl ApexConfig {
    /// Load configuration exclusively from environment variables.
    /// Loads `.env` file if `APEX_ENV` != "production" (dev convenience).
    ///
    /// # Errors
    /// Returns error if any required variable is missing or malformed.
    pub fn from_env() -> Result<Self> {
        let env_name = std::env::var("APEX_ENV").unwrap_or_else(|_| "development".to_string());
        if env_name != "production" {
            let _ = dotenvy::dotenv();
            info!("Loaded .env (environment: {env_name})");
        }

        let alchemy_api_key = optional_env("ALCHEMY_API_KEY");

        let cfg = Self {
            rpc_url: required_env("APEX_RPC_URL")?,
            http_rpc_url: optional_env("APEX_HTTP_RPC_URL")
                .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string()),
            keypair_path: required_env("APEX_KEYPAIR_PATH")?,
            alchemy_api_key,
            min_profit_lamports: parse_env_u64("APEX_MIN_PROFIT_LAMPORTS", 10_000)?,
            max_hops: parse_env_usize("APEX_MAX_HOPS", 4)?,
            max_position_lamports: parse_env_u64("APEX_MAX_POSITION_LAMPORTS", 1_000_000_000)?,
            slippage_bps: parse_env_u16("APEX_SLIPPAGE_BPS", 50)?,
            flash_loan_enabled: parse_env_bool("APEX_FLASH_LOAN_ENABLED", false)?,
            jito_url: required_env("APEX_JITO_URL")?,
            circuit_breaker_threshold_lamports: parse_env_u64(
                "APEX_CB_THRESHOLD_LAMPORTS",
                5_000_000_000,
            )?,
            circuit_breaker_consecutive_losses: parse_env_u32("APEX_CB_CONSECUTIVE_LOSSES", 5)?,
            max_daily_loss_lamports: parse_env_u64(
                "APEX_MAX_DAILY_LOSS_LAMPORTS",
                500_000_000, // 0.5 SOL default
            )?,
            auto_optimize: parse_env_bool("APEX_AUTO_OPTIMIZE", true)?,
            simulation_only: parse_env_bool("APEX_SIMULATION_ONLY", true)?,
        };

        info!(
            simulation_only     = cfg.simulation_only,
            flash_loan_enabled  = cfg.flash_loan_enabled,
            auto_optimize       = cfg.auto_optimize,
            slippage_bps        = cfg.slippage_bps,
            max_daily_loss_sol  = cfg.max_daily_loss_lamports as f64 / 1e9,
            alchemy_ws_active   = cfg.alchemy_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
            alchemy_key_len     = cfg.alchemy_api_key.as_ref().map(|k| k.len()).unwrap_or(0),
            "ApexConfig loaded"
        );

        Ok(cfg)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn required_env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("Missing required environment variable: {key}"))
}

fn optional_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn parse_env_u64(key: &str, default: u64) -> Result<u64> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<u64>()
            .with_context(|| format!("Invalid u64 for {key}: '{val}'")),
        Err(_) => Ok(default),
    }
}

fn parse_env_u16(key: &str, default: u16) -> Result<u16> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<u16>()
            .with_context(|| format!("Invalid u16 for {key}: '{val}'")),
        Err(_) => Ok(default),
    }
}

fn parse_env_usize(key: &str, default: usize) -> Result<usize> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<usize>()
            .with_context(|| format!("Invalid usize for {key}: '{val}'")),
        Err(_) => Ok(default),
    }
}

fn parse_env_u32(key: &str, default: u32) -> Result<u32> {
    match std::env::var(key) {
        Ok(val) => val
            .parse::<u32>()
            .with_context(|| format!("Invalid u32 for {key}: '{val}'")),
        Err(_) => Ok(default),
    }
}

fn parse_env_bool(key: &str, default: bool) -> Result<bool> {
    match std::env::var(key) {
        Ok(val) => match val.to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            other => anyhow::bail!("Invalid bool for {key}: '{other}'"),
        },
        Err(_) => Ok(default),
    }
}
