-- F-008: Speed up stale heartbeat scans
-- Supports queries filtering by status and last_heartbeat_at.

CREATE INDEX IF NOT EXISTS idx_bots_status_last_heartbeat_at
    ON bots (status, last_heartbeat_at);
