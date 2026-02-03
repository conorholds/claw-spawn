-- CRIT-002: Atomic account bot counter table for race-condition-free limit checking
CREATE TABLE IF NOT EXISTS account_bot_counters (
    account_id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    current_count INTEGER NOT NULL DEFAULT 0,
    max_count INTEGER NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Initialize counters for existing accounts
INSERT INTO account_bot_counters (account_id, current_count, max_count)
SELECT 
    a.id,
    COUNT(b.id)::INTEGER,
    a.max_bots
FROM accounts a
LEFT JOIN bots b ON a.id = b.account_id AND b.status != 'destroyed'
GROUP BY a.id, a.max_bots
ON CONFLICT (account_id) DO NOTHING;

-- Create function to atomically increment counter with limit check
CREATE OR REPLACE FUNCTION increment_bot_counter(p_account_id UUID)
RETURNS TABLE (
    success BOOLEAN,
    current_count INTEGER,
    max_count INTEGER
) AS $$
BEGIN
    RETURN QUERY
    UPDATE account_bot_counters
    SET 
        current_count = current_count + 1,
        updated_at = NOW()
    WHERE account_id = p_account_id
      AND current_count < max_count
    RETURNING TRUE, current_count, max_count;
    
    -- If no rows updated, check if counter exists and return current state
    IF NOT FOUND THEN
        RETURN QUERY
        SELECT FALSE, c.current_count, c.max_count
        FROM account_bot_counters c
        WHERE c.account_id = p_account_id;
    END IF;
END;
$$ LANGUAGE plpgsql;

-- Create function to decrement counter (used when bot is destroyed)
CREATE OR REPLACE FUNCTION decrement_bot_counter(p_account_id UUID)
RETURNS VOID AS $$
BEGIN
    UPDATE account_bot_counters
    SET 
        current_count = GREATEST(0, current_count - 1),
        updated_at = NOW()
    WHERE account_id = p_account_id;
END;
$$ LANGUAGE plpgsql;

-- Create function to initialize counter for new account
CREATE OR REPLACE FUNCTION init_account_counter()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO account_bot_counters (account_id, current_count, max_count)
    VALUES (NEW.id, 0, NEW.max_bots);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to auto-create counter when account is created
DROP TRIGGER IF EXISTS init_account_counter_trigger ON accounts;
CREATE TRIGGER init_account_counter_trigger
    AFTER INSERT ON accounts
    FOR EACH ROW
    EXECUTE FUNCTION init_account_counter();

-- Trigger to update max_count when account subscription changes
CREATE OR REPLACE FUNCTION update_account_counter_max()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE account_bot_counters
    SET max_count = NEW.max_bots, updated_at = NOW()
    WHERE account_id = NEW.id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS update_account_counter_max_trigger ON accounts;
CREATE TRIGGER update_account_counter_max_trigger
    AFTER UPDATE OF max_bots ON accounts
    FOR EACH ROW
    EXECUTE FUNCTION update_account_counter_max();
