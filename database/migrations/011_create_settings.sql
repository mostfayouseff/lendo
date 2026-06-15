-- Migration 011: Key-value settings store (dashboard-configurable)
CREATE TABLE settings (
    key         VARCHAR(128) PRIMARY KEY,
    value       JSONB        NOT NULL,
    description TEXT,
    updated_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Default platform settings
INSERT INTO settings (key, value, description) VALUES
    ('bot.enabled',                 'false',   'Whether the trading bot is running'),
    ('bot.mode',                    '"test"',  'live or test'),
    ('bot.min_profit_lamports',     '10000',   'Minimum profit threshold in lamports'),
    ('bot.max_position_lamports',   '1000000000', 'Max position size per trade'),
    ('bot.slippage_bps',            '50',      'Slippage tolerance in basis points'),
    ('bot.max_hops',                '4',       'Maximum arbitrage path hops'),
    ('bot.flash_loan_enabled',      'false',   'Enable flash loan execution'),
    ('bot.flash_loan_provider',     '"solend"','Flash loan provider'),
    ('bot.jito_tip_lamports',       '1000',    'Jito bundle tip in lamports'),
    ('risk.max_daily_loss_lamports','500000000','Circuit breaker: daily loss limit'),
    ('risk.max_consecutive_losses', '10',      'Circuit breaker: consecutive loss limit'),
    ('risk.circuit_breaker_enabled','true',    'Enable circuit breaker'),
    ('monitoring.prometheus_enabled','true',   'Enable Prometheus metrics'),
    ('monitoring.log_level',        '"info"',  'Logging verbosity');
