-- Convert INET columns to TEXT to avoid requiring the sqlx ipnetwork feature.
-- IP validation is handled at the application layer.
ALTER TABLE sessions   ALTER COLUMN ip_address TYPE TEXT USING ip_address::text;
ALTER TABLE audit_logs ALTER COLUMN ip_address TYPE TEXT USING ip_address::text;
