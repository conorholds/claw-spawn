CREATE TABLE IF NOT EXISTS accounts (
    id UUID PRIMARY KEY,
    external_id VARCHAR(255) NOT NULL UNIQUE,
    subscription_tier VARCHAR(50) NOT NULL DEFAULT 'free',
    max_bots INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_accounts_external_id ON accounts(external_id);

CREATE TABLE IF NOT EXISTS bots (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    persona VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    droplet_id BIGINT,
    desired_config_version_id UUID,
    applied_config_version_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_heartbeat_at TIMESTAMPTZ
);

CREATE INDEX idx_bots_account_id ON bots(account_id);
CREATE INDEX idx_bots_status ON bots(status);
CREATE INDEX idx_bots_droplet_id ON bots(droplet_id);

CREATE TABLE IF NOT EXISTS bot_configs (
    id UUID PRIMARY KEY,
    bot_id UUID NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    trading_config JSONB NOT NULL,
    risk_config JSONB NOT NULL,
    secrets_encrypted BYTEA NOT NULL,
    llm_provider VARCHAR(100) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(bot_id, version)
);

CREATE INDEX idx_bot_configs_bot_id ON bot_configs(bot_id);

CREATE TABLE IF NOT EXISTS droplets (
    id BIGINT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    region VARCHAR(50) NOT NULL,
    size VARCHAR(50) NOT NULL,
    image VARCHAR(100) NOT NULL,
    status VARCHAR(50) NOT NULL,
    ip_address INET,
    bot_id UUID REFERENCES bots(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    destroyed_at TIMESTAMPTZ
);

CREATE INDEX idx_droplets_bot_id ON droplets(bot_id);

CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_accounts_updated_at BEFORE UPDATE ON accounts
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_bots_updated_at BEFORE UPDATE ON bots
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
