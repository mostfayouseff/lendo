-- Migration 007: Executed trades
CREATE TYPE trade_status AS ENUM ('pending', 'simulating', 'signed', 'submitted', 'confirmed', 'failed', 'reverted');

CREATE TABLE trades (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    opportunity_id          UUID REFERENCES opportunities(id) ON DELETE SET NULL,
    strategy_id             UUID REFERENCES strategies(id) ON DELETE SET NULL,
    wallet_id               UUID REFERENCES wallets(id) ON DELETE SET NULL,
    status                  trade_status NOT NULL DEFAULT 'pending',
    signature               VARCHAR(128),
    input_mint              VARCHAR(64)  NOT NULL,
    output_mint             VARCHAR(64)  NOT NULL,
    input_amount_lamports   BIGINT NOT NULL,
    output_amount_lamports  BIGINT,
    expected_profit_lamports BIGINT NOT NULL,
    actual_profit_lamports  BIGINT,
    fee_lamports            BIGINT NOT NULL DEFAULT 0,
    jito_tip_lamports       BIGINT NOT NULL DEFAULT 0,
    flash_loan_fee_lamports BIGINT NOT NULL DEFAULT 0,
    slippage_bps            SMALLINT,
    hop_count               SMALLINT NOT NULL DEFAULT 1,
    dex_path                TEXT NOT NULL,
    simulation_passed       BOOLEAN,
    error_message           TEXT,
    slot                    BIGINT,
    block_time              TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    confirmed_at            TIMESTAMPTZ,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_trades_strategy_id    ON trades(strategy_id);
CREATE INDEX idx_trades_wallet_id      ON trades(wallet_id);
CREATE INDEX idx_trades_status         ON trades(status);
CREATE INDEX idx_trades_signature      ON trades(signature);
CREATE INDEX idx_trades_created_at     ON trades(created_at DESC);
CREATE INDEX idx_trades_profit         ON trades(actual_profit_lamports DESC NULLS LAST);

CREATE TRIGGER trg_trades_updated_at
    BEFORE UPDATE ON trades
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();
