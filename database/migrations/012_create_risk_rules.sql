-- Migration 012: Risk rules (blacklists, exposure limits, etc.)
CREATE TYPE risk_rule_type AS ENUM (
    'pool_blacklist', 'token_blacklist', 'dex_blacklist',
    'max_daily_loss', 'max_trade_size', 'max_slippage',
    'max_consecutive_losses', 'wallet_exposure_limit'
);

CREATE TABLE risk_rules (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_type   risk_rule_type NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    name        VARCHAR(128) NOT NULL,
    description TEXT,
    config      JSONB NOT NULL DEFAULT '{}',
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_risk_rules_type    ON risk_rules(rule_type);
CREATE INDEX idx_risk_rules_enabled ON risk_rules(enabled);

CREATE TRIGGER trg_risk_rules_updated_at
    BEFORE UPDATE ON risk_rules
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

-- Seed default rules
INSERT INTO risk_rules (rule_type, name, description, config) VALUES
    ('max_daily_loss',         'Daily Loss Limit',       'Stop trading when daily loss exceeds threshold', '{"threshold_lamports": 500000000}'),
    ('max_consecutive_losses', 'Consecutive Loss Limit', 'Pause after N consecutive losing trades',        '{"max_losses": 10}'),
    ('max_trade_size',         'Max Trade Size',         'Reject trades above this position size',         '{"max_lamports": 1000000000}'),
    ('max_slippage',           'Max Slippage',           'Reject paths with slippage above threshold',     '{"max_bps": 100}');
