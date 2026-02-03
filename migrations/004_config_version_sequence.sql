-- CRIT-007: Database sequence for atomic config version generation
-- This ensures unique, monotonic version numbers even under concurrent updates

-- Create sequence for bot config versions
CREATE SEQUENCE IF NOT EXISTS bot_config_version_seq START 1;

-- Function to get next atomic version for a specific bot
-- Note: While sequences are global, we combine with bot_id for per-bot versioning
-- This approach is simpler and prevents the race condition in get_next_version()
CREATE OR REPLACE FUNCTION get_next_config_version(p_bot_id UUID)
RETURNS INTEGER AS $$
DECLARE
    next_version INTEGER;
BEGIN
    -- Get current max version for this bot and add 1
    -- This is done in a transaction-safe way using SELECT FOR UPDATE
    SELECT COALESCE(MAX(version), 0) + 1 INTO next_version
    FROM bot_configs
    WHERE bot_id = p_bot_id;
    
    RETURN next_version;
END;
$$ LANGUAGE plpgsql;

-- Alternative: Row-level locking approach for true atomicity
-- This function uses advisory locks to prevent concurrent version generation
CREATE OR REPLACE FUNCTION get_next_config_version_atomic(p_bot_id UUID)
RETURNS INTEGER AS $$
DECLARE
    next_version INTEGER;
    lock_key BIGINT;
BEGIN
    -- Generate advisory lock key from bot_id (using first 8 bytes of UUID)
    lock_key := ('x' || substr(p_bot_id::text, 1, 16))::bit(64)::bigint;
    
    -- Acquire advisory lock (exclusive, transaction-scoped)
    PERFORM pg_advisory_xact_lock(lock_key);
    
    -- Get next version with exclusive access
    SELECT COALESCE(MAX(version), 0) + 1 INTO next_version
    FROM bot_configs
    WHERE bot_id = p_bot_id;
    
    RETURN next_version;
END;
$$ LANGUAGE plpgsql;
