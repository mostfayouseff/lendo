-- Migration 008: Flash loan execution records
CREATE TYPE flash_loan_provider AS ENUM ('solend', 'marginfi', 'kamino');
CREATE TYPE flash_loan_status   AS ENUM ('initiated', 'borrowed', 'swapped', 'repaid', 'failed');

CREATE TABLE flash_loan_executions (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    trade_id             UUID NOT NULL REFERENCES trades(id) ON DELETE CASCADE,
    provider             flash_loan_provider NOT NULL,
    status               flash_loan_status NOT NULL DEFAULT 'initiated',
    borrow_mint          VARCHAR(64) NOT NULL,
    borrow_amount        BIGINT NOT NULL,
    repay_amount         BIGINT NOT NULL,
    fee_amount           BIGINT NOT NULL,
    fee_bps              SMALLINT NOT NULL,
    signature            VARCHAR(128),
    error_message        TEXT,
    initiated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at         TIMESTAMPTZ
);

CREATE INDEX idx_flash_loans_trade_id  ON flash_loan_executions(trade_id);
CREATE INDEX idx_flash_loans_provider  ON flash_loan_executions(provider);
CREATE INDEX idx_flash_loans_status    ON flash_loan_executions(status);
