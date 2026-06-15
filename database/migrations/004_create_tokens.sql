-- Migration 004: Token universe
CREATE TYPE token_status AS ENUM ('active', 'disabled', 'blacklisted');

CREATE TABLE tokens (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    mint_address   VARCHAR(64)  UNIQUE NOT NULL,
    symbol         VARCHAR(32)  NOT NULL,
    name           VARCHAR(128) NOT NULL,
    decimals       SMALLINT     NOT NULL,
    status         token_status NOT NULL DEFAULT 'active',
    logo_uri       TEXT,
    coingecko_id   VARCHAR(128),
    liquidity_usd  NUMERIC(20, 2),
    verified       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_tokens_mint_address ON tokens(mint_address);
CREATE INDEX idx_tokens_symbol       ON tokens(symbol);
CREATE INDEX idx_tokens_status       ON tokens(status);

CREATE TRIGGER trg_tokens_updated_at
    BEFORE UPDATE ON tokens
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();
