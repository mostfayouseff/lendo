// =============================================================================
// APEX-MEV CONFIGURATION — Live Trading Mode
//
// All configuration loaded exclusively from environment variables.
// Helius WebSocket is the PRIMARY ingress source.
// Alchemy WebSocket is the FALLBACK ingress source.
// Jupiter price monitor is always active with self-healing fallbacks.
// =============================================================================

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::info;

/// Top-level bot configuration.
/// Secret fields intentionally omit `Serialize` to prevent secret leakage in logs.
#[derive(Debug, Deserialize, Clone)]
pub struct ApexConfig {
    // ── Network ──────────────────────────────────────────────────────────────
    /// Solana RPC WebSocket URL
    pub rpc_url: String,

    /// Solana RPC HTTP URL for JSON-RPC calls (getLatestBlockhash, getBalance, etc.)
    pub http_rpc_url: String,

    /// Path to the operator keypair JSON file
    pub keypair_path: String,

    // ── Helius (PRIMARY stream) ────────────────────────────────────────────────
    /// Helius API key for PRIMARY WebSocket stream (transactionSubscribe + logsSubscribe)
    /// Source: HELIUS_API_KEY env var
    pub helius_api_key: Option<String>,

    // ── Alchemy (FALLBACK stream) ─────────────────────────────────────────────
    /// Alchemy API key for FALLBACK WebSocket stream (logsSubscribe)
    /// Source: ALCHEMY_API_KEY env var
    pub alchemy_api_key: Option<String>,

    // ── Jupiter ───────────────────────────────────────────────────────────────
    /// Jupiter API key for authenticated Jupiter Ultra order endpoint
    /// Source: JUPITER_API_KEY env var (validated as required by the bot)
    pub jupiter_api_key: Option<String>,

    // ── Strategy ─────────────────────────────────────────────────────────────
    /// Minimum expected profit in lamports (0 = no minimum, always attempt)
    pub min_profit_lamports: u64,

    /// Maximum number of hops per arbitrage path
    pub max_hops: usize,

    /// Maximum capital to deploy per trade in lamports
    pub max_position_lamports: u64,

    /// Slippage tolerance in basis points (50 = 0.5%)
    pub slippage_bps: u16,

    // ── Flash Loans ───────────────────────────────────────────────────────────
    /// Use Solend flash loans for capital-free arbitrage
    pub flash_loan_enabled: bool,

    // ── Jito ─────────────────────────────────────────────────────────────────
    /// Jito block engine endpoint
    pub jito_url: String,

    /// Jito tip account used for every sent swap transaction
    pub jito_tip_account: String,

    /// Configurable minimal tip amount in lamports
    pub jito_tip_lamports: u64,

    // ── Risk ─────────────────────────────────────────────────────────────────
    /// Maximum drawdown before circuit breaker trips (in lamports)
    pub circuit_breaker_threshold_lamports: u64,

    /// Number of consecutive losses before circuit breaker activates
    pub circuit_breaker_consecutive_losses: u32,

    /// Maximum total loss allowed per session (in lamports)
    pub max_daily_loss_lamports: u64,

    // ── Self-Optimization ─────────────────────────────────────────────────────
    /// Automatically adjust slippage and position sizing based on performance
    pub auto_optimize: bool,

    // ── Simulation ───────────────────────────────────────────────────────────
    /// Whether to run in simulation-only mode (never sends real transactions)
    pub simulation_only: bool,
}

impl ApexConfig {
    /// Load configuration exclusively from environment variables.
    pub fn from_env() -> Result<Self> {
        let env_name = std::env::var("APEX_ENV").unwrap_or_else(|_| "development".to_string());
        if env_name != "production" {
            let _ = dotenvy::dotenv();
            info!("Loaded .env (environment: {env_name})");
        }

        let helius_api_key = optional_env("HELIUS_API_KEY");
        let alchemy_api_key = optional_env("ALCHEMY_API_KEY");
        let jupiter_api_key = optional_env("JUPITER_API_KEY");

        let simulation_only = parse_env_bool("APEX_SIMULATION_ONLY", false)?;

        let cfg = Self {
            rpc_url: optional_env("APEX_RPC_URL")
                .unwrap_or_else(|| {
                    if let Some(ref key) = helius_api_key {
                        format!("wss://mainnet.helius-rpc.com/?api-key={}", key)
                    } else {
                        "wss://api.mainnet-beta.solana.com".to_string()
                    }
                }),
            http_rpc_url: optional_env("APEX_HTTP_RPC_URL")
                .unwrap_or_else(|| {
                    if let Some(ref key) = helius_api_key {
                        format!("https://mainnet.helius-rpc.com/?api-key={}", key)
                    } else {
                        "https://api.mainnet-beta.solana.com".to_string()
                    }
                }),
            keypair_path: optional_env("APEX_KEYPAIR_PATH")
                .unwrap_or_else(|| "live-key.json".to_string()),
            helius_api_key: helius_api_key.clone(),
            alchemy_api_key,
            jupiter_api_key,
            min_profit_lamports: parse_env_u64("APEX_MIN_PROFIT_LAMPORTS", 0)?,
            max_hops: parse_env_usize("APEX_MAX_HOPS", 4)?,
            max_position_lamports: parse_env_u64("APEX_MAX_POSITION_LAMPORTS", 1_000_000_000)?,
            slippage_bps: parse_env_u16("APEX_SLIPPAGE_BPS", 50)?,
            flash_loan_enabled: parse_env_bool("APEX_FLASH_LOAN_ENABLED", true)?,
            jito_url: optional_env("APEX_JITO_URL")
                .unwrap_or_else(|| "https://mainnet.block-engine.jito.wtf".to_string()),
            jito_tip_account: required_env("JITO_TIP_ACCOUNT")?,
            jito_tip_lamports: parse_env_u64("JITO_TIP_LAMPORTS", 1_000)?,
            circuit_breaker_threshold_lamports: parse_env_u64(
                "APEX_CB_THRESHOLD_LAMPORTS",
                5_000_000_000,
            )?,
            circuit_breaker_consecutive_losses: parse_env_u32("APEX_CB_CONSECUTIVE_LOSSES", 10)?,
            max_daily_loss_lamports: parse_env_u64(
                "APEX_MAX_DAILY_LOSS_LAMPORTS",
                500_000_000,
            )?,
            auto_optimize: parse_env_bool("APEX_AUTO_OPTIMIZE", true)?,
            simulation_only,
        };

        info!(
            simulation_only     = cfg.simulation_only,
            flash_loan_enabled  = cfg.flash_loan_enabled,
            auto_optimize       = cfg.auto_optimize,
            slippage_bps        = cfg.slippage_bps,
            min_profit_lamports = cfg.min_profit_lamports,
            max_daily_loss_sol  = cfg.max_daily_loss_lamports as f64 / 1e9,
            helius_active       = cfg.helius_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
            alchemy_active      = cfg.alchemy_api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
            jupiter_key_active  = cfg.jupiter_api_key.is_some(),
            jito_tip_lamports   = cfg.jito_tip_lamports,
            "ApexConfig loaded — LIVE TRADING MODE"
        );

        Ok(cfg)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn optional_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn required_env(key: &str) -> Result<String> {
    optional_env(key).ok_or_else(|| anyhow::anyhow!("Missing required environment variable: {key}"))
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
