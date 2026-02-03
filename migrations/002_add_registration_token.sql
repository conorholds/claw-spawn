-- CRIT-001: Add registration_token column for bot authentication
ALTER TABLE bots ADD COLUMN IF NOT EXISTS registration_token VARCHAR(255);

CREATE INDEX IF NOT EXISTS idx_bots_registration_token ON bots(registration_token);
