-- F-007: Speed up account bot pagination ordering
-- Supports queries that filter by account_id and order by created_at DESC.

CREATE INDEX IF NOT EXISTS idx_bots_account_created_at_desc
    ON bots (account_id, created_at DESC);
