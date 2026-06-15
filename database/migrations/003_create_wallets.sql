-- Migration 003: Operator wallets
CREATE TYPE wallet_status AS ENUM ('active', 'inactive', 'locked');
CREATE TYPE wallet_type   AS ENUM ('keypair_json', 'private_key', 'seed_phrase');

CREATE TABLE wallets (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label             VARCHAR(128) NOT NULL,
    address           VARCHAR(64)  NOT NULL,
    wallet_type       wallet_type  NOT NULL DEFAULT 'keypair_json',
    encrypted_secret  TEXT         NOT NULL,
    status            wallet_status NOT NULL DEFAULT 'inactive',
    is_active         BOOLEAN      NOT NULL DEFAULT FALSE,
    balance_lamports  BIGINT       NOT NULL DEFAULT 0,
    balance_updated_at TIMESTAMPTZ,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_wallets_active_per_user ON wallets(user_id) WHERE is_active = TRUE;
CREATE INDEX idx_wallets_user_id  ON wallets(user_id);
CREATE INDEX idx_wallets_address  ON wallets(address);
CREATE INDEX idx_wallets_status   ON wallets(status);

CREATE TRIGGER trg_wallets_updated_at
    BEFORE UPDATE ON wallets
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();
