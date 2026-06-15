-- Migration 009: Audit log for all user and system actions
CREATE TYPE audit_action AS ENUM (
    'user_login', 'user_logout', 'user_created', 'user_updated', 'user_deleted',
    'wallet_added', 'wallet_activated', 'wallet_deleted',
    'token_added', 'token_enabled', 'token_disabled', 'token_deleted',
    'strategy_created', 'strategy_updated', 'strategy_deleted',
    'strategy_started', 'strategy_paused',
    'bot_started', 'bot_stopped', 'bot_paused', 'bot_resumed', 'emergency_stop',
    'settings_updated', 'risk_rule_updated',
    'trade_executed', 'trade_failed'
);

CREATE TABLE audit_logs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    action      audit_action NOT NULL,
    entity_type VARCHAR(64),
    entity_id   UUID,
    old_value   JSONB,
    new_value   JSONB,
    ip_address  INET,
    user_agent  TEXT,
    metadata    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_logs_user_id    ON audit_logs(user_id);
CREATE INDEX idx_audit_logs_action     ON audit_logs(action);
CREATE INDEX idx_audit_logs_entity     ON audit_logs(entity_type, entity_id);
CREATE INDEX idx_audit_logs_created_at ON audit_logs(created_at DESC);
