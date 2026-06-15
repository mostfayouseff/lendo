-- Migration 006: Detected arbitrage opportunities
CREATE TYPE opportunity_status AS ENUM ('detected', 'simulating', 'executing', 'executed', 'skipped', 'failed');

CREATE TABLE opportunities (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    strategy_id           UUID REFERENCES strategies(id) ON DELETE SET NULL,
    status                opportunity_status NOT NULL DEFAULT 'detected',
    path                  TEXT[]  NOT NULL,
    dex_path              TEXT    NOT NULL,
    input_mint            VARCHAR(64) NOT NULL,
    output_mint           VARCHAR(64) NOT NULL,
    input_amount_lamports BIGINT  NOT NULL,
    estimated_profit_lamports BIGINT NOT NULL,
    estimated_profit_usd  NUMERIC(12, 6),
    price_impact_pct      NUMERIC(8, 4),
    hop_count             SMALLINT NOT NULL,
    gnn_confidence        NUMERIC(5, 4),
    skip_reason           TEXT,
    detected_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    executed_at           TIMESTAMPTZ
);

CREATE INDEX idx_opportunities_strategy_id  ON opportunities(strategy_id);
CREATE INDEX idx_opportunities_status       ON opportunities(status);
CREATE INDEX idx_opportunities_detected_at  ON opportunities(detected_at DESC);
CREATE INDEX idx_opportunities_profit       ON opportunities(estimated_profit_lamports DESC);
