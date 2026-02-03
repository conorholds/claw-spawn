-- F-002: Fix advisory-lock key generation for atomic config versioning
--
-- The previous implementation attempted to derive a bigint lock key from a UUID
-- text substring, which can include '-' and is not valid hex. Use a stable hash
-- of the UUID text to produce a bigint key for pg_advisory_xact_lock.

CREATE OR REPLACE FUNCTION get_next_config_version_atomic(p_bot_id UUID)
RETURNS INTEGER AS $$
DECLARE
    next_version INTEGER;
    lock_key BIGINT;
BEGIN
    -- Stable bigint key derived from bot_id
    lock_key := hashtextextended(p_bot_id::text, 0);

    -- Acquire advisory lock (exclusive, transaction-scoped)
    PERFORM pg_advisory_xact_lock(lock_key);

    -- Get next version with exclusive access
    SELECT COALESCE(MAX(version), 0) + 1 INTO next_version
    FROM bot_configs
    WHERE bot_id = p_bot_id;

    RETURN next_version;
END;
$$ LANGUAGE plpgsql;
