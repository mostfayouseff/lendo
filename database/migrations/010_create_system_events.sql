-- Migration 010: System events (circuit breaker trips, RPC failures, etc.)
CREATE TYPE event_severity AS ENUM ('debug', 'info', 'warning', 'error', 'critical');
CREATE TYPE event_category AS ENUM (
    'rpc', 'ingress', 'trading', 'risk', 'circuit_breaker',
    'flash_loan', 'wallet', 'monitoring', 'system'
);

CREATE TABLE system_events (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    severity   event_severity  NOT NULL DEFAULT 'info',
    category   event_category  NOT NULL DEFAULT 'system',
    title      VARCHAR(256)    NOT NULL,
    message    TEXT            NOT NULL,
    metadata   JSONB           NOT NULL DEFAULT '{}',
    resolved   BOOLEAN         NOT NULL DEFAULT FALSE,
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ     NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_system_events_severity   ON system_events(severity);
CREATE INDEX idx_system_events_category   ON system_events(category);
CREATE INDEX idx_system_events_created_at ON system_events(created_at DESC);
CREATE INDEX idx_system_events_unresolved ON system_events(created_at DESC) WHERE NOT resolved;
