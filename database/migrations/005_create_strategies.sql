-- Migration 005: Trading strategies
CREATE TYPE strategy_type   AS ENUM ('cross_dex', 'triangular', 'multi_hop', 'flash_loan', 'jupiter_route');
CREATE TYPE strategy_status AS ENUM ('active', 'paused', 'disabled');

CREATE TABLE strategies (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                   VARCHAR(128) NOT NULL,
    strategy_type          strategy_type   NOT NULL,
    status                 strategy_status NOT NULL DEFAULT 'paused',
    min_profit_lamports    BIGINT  NOT NULL DEFAULT 10000,
    max_position_lamports  BIGINT  NOT NULL DEFAULT 1000000000,
    max_slippage_bps       SMALLINT NOT NULL DEFAULT 50,
    max_hops               SMALLINT NOT NULL DEFAULT 4,
    flash_loan_enabled     BOOLEAN NOT NULL DEFAULT FALSE,
    flash_loan_provider    VARCHAR(32),
    dex_whitelist          TEXT[]  NOT NULL DEFAULT '{}',
    token_whitelist        TEXT[]  NOT NULL DEFAULT '{}',
    config                 JSONB   NOT NULL DEFAULT '{}',
    trades_executed        BIGINT  NOT NULL DEFAULT 0,
    total_profit_lamports  BIGINT  NOT NULL DEFAULT 0,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_strategies_user_id ON strategies(user_id);
CREATE INDEX idx_strategies_status  ON strategies(status);
CREATE INDEX idx_strategies_type    ON strategies(strategy_type);

CREATE TRIGGER trg_strategies_updated_at
    BEFORE UPDATE ON strategies
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();
