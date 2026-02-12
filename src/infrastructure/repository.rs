use crate::domain::{Account, Bot, BotStatus, Droplet, Persona, StoredBotConfig, SubscriptionTier};
use async_trait::async_trait;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

#[async_trait]
pub trait AccountRepository: Send + Sync {
    #[must_use]
    async fn create(&self, account: &Account) -> Result<(), RepositoryError>;
    #[must_use]
    async fn get_by_id(&self, id: Uuid) -> Result<Account, RepositoryError>;
    #[must_use]
    async fn get_by_external_id(&self, external_id: &str) -> Result<Account, RepositoryError>;
    #[must_use]
    async fn update_subscription(
        &self,
        id: Uuid,
        tier: SubscriptionTier,
    ) -> Result<(), RepositoryError>;
}

#[async_trait]
pub trait BotRepository: Send + Sync {
    #[must_use]
    async fn create(&self, bot: &Bot) -> Result<(), RepositoryError>;
    #[must_use]
    async fn get_by_id(&self, id: Uuid) -> Result<Bot, RepositoryError>;
    #[must_use]
    async fn get_by_id_with_token(&self, id: Uuid, token: &str) -> Result<Bot, RepositoryError>;
    #[must_use]
    async fn list_by_account(&self, account_id: Uuid) -> Result<Vec<Bot>, RepositoryError>;
    /// PERF-002: Paginated list of bots for account
    /// Use limit/offset for pagination instead of loading all bots
    #[must_use]
    async fn list_by_account_paginated(
        &self,
        account_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Bot>, RepositoryError>;
    /// PERF-001: Count bots for account without fetching all rows
    /// Use SQL COUNT(*) instead of list_by_account().len()
    #[must_use]
    async fn count_by_account(&self, account_id: Uuid) -> Result<i64, RepositoryError>;
    #[must_use]
    async fn update_status(&self, id: Uuid, status: BotStatus) -> Result<(), RepositoryError>;
    #[must_use]
    async fn update_droplet(
        &self,
        bot_id: Uuid,
        droplet_id: Option<i64>,
    ) -> Result<(), RepositoryError>;
    #[must_use]
    async fn update_config_version(
        &self,
        bot_id: Uuid,
        desired: Option<Uuid>,
        applied: Option<Uuid>,
    ) -> Result<(), RepositoryError>;
    #[must_use]
    async fn update_heartbeat(&self, bot_id: Uuid) -> Result<(), RepositoryError>;
    #[must_use]
    async fn update_registration_token(
        &self,
        bot_id: Uuid,
        token: &str,
    ) -> Result<(), RepositoryError>;
    #[must_use]
    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError>;
    #[must_use]
    async fn hard_delete(&self, id: Uuid) -> Result<(), RepositoryError>;
    /// Atomically increment bot counter for account, returning (success, current_count, max_count)
    /// CRIT-002: Prevents race conditions in account limit checking
    #[must_use]
    async fn increment_bot_counter(
        &self,
        account_id: Uuid,
    ) -> Result<(bool, i32, i32), RepositoryError>;
    /// Decrement bot counter when bot is destroyed
    #[must_use]
    async fn decrement_bot_counter(&self, account_id: Uuid) -> Result<(), RepositoryError>;
    /// List bots with stale heartbeats (HIGH-001)
    #[must_use]
    async fn list_stale_bots(
        &self,
        threshold: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<Bot>, RepositoryError>;
}

#[async_trait]
pub trait ConfigRepository: Send + Sync {
    #[must_use]
    async fn create(&self, config: &StoredBotConfig) -> Result<(), RepositoryError>;
    #[must_use]
    async fn get_by_id(&self, id: Uuid) -> Result<StoredBotConfig, RepositoryError>;
    #[must_use]
    async fn get_latest_for_bot(
        &self,
        bot_id: Uuid,
    ) -> Result<Option<StoredBotConfig>, RepositoryError>;
    #[must_use]
    async fn list_by_bot(&self, bot_id: Uuid) -> Result<Vec<StoredBotConfig>, RepositoryError>;
    /// Get next config version atomically using advisory locks
    /// CRIT-007: Prevents duplicate version numbers under concurrent updates
    #[must_use]
    async fn get_next_version_atomic(&self, bot_id: Uuid) -> Result<i32, RepositoryError>;
}

#[async_trait]
pub trait DropletRepository: Send + Sync {
    #[must_use]
    async fn create(&self, droplet: &Droplet) -> Result<(), RepositoryError>;
    #[must_use]
    async fn get_by_id(&self, id: i64) -> Result<Droplet, RepositoryError>;
    #[must_use]
    async fn update_bot_assignment(
        &self,
        droplet_id: i64,
        bot_id: Option<Uuid>,
    ) -> Result<(), RepositoryError>;
    #[must_use]
    async fn update_status(&self, droplet_id: i64, status: &str) -> Result<(), RepositoryError>;
    #[must_use]
    async fn update_ip(&self, droplet_id: i64, ip: Option<String>) -> Result<(), RepositoryError>;
    #[must_use]
    async fn mark_destroyed(&self, droplet_id: i64) -> Result<(), RepositoryError>;
}

pub struct PostgresAccountRepository {
    pool: PgPool,
}

impl PostgresAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AccountRepository for PostgresAccountRepository {
    async fn create(&self, account: &Account) -> Result<(), RepositoryError> {
        let tier_str = match account.subscription_tier {
            SubscriptionTier::Free => "free",
            SubscriptionTier::Basic => "basic",
            SubscriptionTier::Pro => "pro",
        };

        sqlx::query(
            r#"
            INSERT INTO accounts (id, external_id, subscription_tier, max_bots, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(account.id)
        .bind(&account.external_id)
        .bind(tier_str)
        .bind(account.max_bots)
        .bind(account.created_at)
        .bind(account.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<Account, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, external_id, subscription_tier, max_bots, created_at, updated_at
            FROM accounts
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => RepositoryError::NotFound(format!("Account {}", id)),
            _ => RepositoryError::DatabaseError(e),
        })?;

        Ok(row_to_account(&row)?)
    }

    async fn get_by_external_id(&self, external_id: &str) -> Result<Account, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, external_id, subscription_tier, max_bots, created_at, updated_at
            FROM accounts
            WHERE external_id = $1
            "#,
        )
        .bind(external_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                RepositoryError::NotFound(format!("Account {}", external_id))
            }
            _ => RepositoryError::DatabaseError(e),
        })?;

        Ok(row_to_account(&row)?)
    }

    async fn update_subscription(
        &self,
        id: Uuid,
        tier: SubscriptionTier,
    ) -> Result<(), RepositoryError> {
        let tier_str = match tier {
            SubscriptionTier::Free => "free",
            SubscriptionTier::Basic => "basic",
            SubscriptionTier::Pro => "pro",
        };

        let max_bots = match tier {
            SubscriptionTier::Free => 0,
            SubscriptionTier::Basic => 2,
            SubscriptionTier::Pro => 4,
        };

        sqlx::query(
            r#"
            UPDATE accounts
            SET subscription_tier = $1, max_bots = $2, updated_at = $3
            WHERE id = $4
            "#,
        )
        .bind(tier_str)
        .bind(max_bots)
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

fn row_to_account(row: &sqlx::postgres::PgRow) -> Result<Account, RepositoryError> {
    let tier_str: String = row.try_get("subscription_tier")?;
    let tier = match tier_str.as_str() {
        "free" => SubscriptionTier::Free,
        "basic" => SubscriptionTier::Basic,
        "pro" => SubscriptionTier::Pro,
        _ => {
            return Err(RepositoryError::InvalidData(format!(
                "Unknown tier: {}",
                tier_str
            )))
        }
    };

    Ok(Account {
        id: row.try_get("id")?,
        external_id: row.try_get("external_id")?,
        subscription_tier: tier,
        max_bots: row.try_get("max_bots")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub struct PostgresBotRepository {
    pool: PgPool,
}

impl PostgresBotRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn hash_registration_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    format!("sha256:{:x}", digest)
}

#[async_trait]
impl BotRepository for PostgresBotRepository {
    async fn create(&self, bot: &Bot) -> Result<(), RepositoryError> {
        let status_str = bot.status.to_string();
        let persona_str = bot.persona.to_string();

        sqlx::query(
            r#"
            INSERT INTO bots (id, account_id, name, persona, status, droplet_id, 
                             desired_config_version_id, applied_config_version_id, 
                             registration_token, created_at, updated_at, last_heartbeat_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(bot.id)
        .bind(bot.account_id)
        .bind(&bot.name)
        .bind(persona_str)
        .bind(status_str)
        .bind(bot.droplet_id)
        .bind(bot.desired_config_version_id)
        .bind(bot.applied_config_version_id)
        .bind(&bot.registration_token)
        .bind(bot.created_at)
        .bind(bot.updated_at)
        .bind(bot.last_heartbeat_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<Bot, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, account_id, name, persona, status, droplet_id,
                   desired_config_version_id, applied_config_version_id,
                   registration_token, created_at, updated_at, last_heartbeat_at
            FROM bots
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => RepositoryError::NotFound(format!("Bot {}", id)),
            _ => RepositoryError::DatabaseError(e),
        })?;

        Ok(row_to_bot(&row)?)
    }

    async fn get_by_id_with_token(&self, id: Uuid, token: &str) -> Result<Bot, RepositoryError> {
        let hashed_token = hash_registration_token(token);
        let row = sqlx::query(
            r#"
            SELECT id, account_id, name, persona, status, droplet_id,
                   desired_config_version_id, applied_config_version_id,
                   registration_token, created_at, updated_at, last_heartbeat_at
            FROM bots
            WHERE id = $1
              AND (registration_token = $2 OR registration_token = $3)
            "#,
        )
        .bind(id)
        .bind(token)
        .bind(hashed_token)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                RepositoryError::NotFound(format!("Bot {} with invalid token", id))
            }
            _ => RepositoryError::DatabaseError(e),
        })?;

        Ok(row_to_bot(&row)?)
    }

    async fn list_by_account(&self, account_id: Uuid) -> Result<Vec<Bot>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, account_id, name, persona, status, droplet_id,
                   desired_config_version_id, applied_config_version_id,
                   registration_token, created_at, updated_at, last_heartbeat_at
            FROM bots
            WHERE account_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_bot).collect()
    }

    async fn count_by_account(&self, account_id: Uuid) -> Result<i64, RepositoryError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM bots 
            WHERE account_id = $1
            "#,
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

    async fn list_by_account_paginated(
        &self,
        account_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Bot>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, account_id, name, persona, status, droplet_id,
                   desired_config_version_id, applied_config_version_id,
                   registration_token, created_at, updated_at, last_heartbeat_at
            FROM bots
            WHERE account_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(account_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_bot).collect()
    }

    async fn update_status(&self, id: Uuid, status: BotStatus) -> Result<(), RepositoryError> {
        let status_str = status.to_string();

        sqlx::query(
            r#"
            UPDATE bots
            SET status = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(status_str)
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_droplet(
        &self,
        bot_id: Uuid,
        droplet_id: Option<i64>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE bots
            SET droplet_id = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(droplet_id)
        .bind(Utc::now())
        .bind(bot_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_config_version(
        &self,
        bot_id: Uuid,
        desired: Option<Uuid>,
        applied: Option<Uuid>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE bots
            SET desired_config_version_id = $1, applied_config_version_id = $2, updated_at = $3
            WHERE id = $4
            "#,
        )
        .bind(desired)
        .bind(applied)
        .bind(Utc::now())
        .bind(bot_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_heartbeat(&self, bot_id: Uuid) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE bots
            SET last_heartbeat_at = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(bot_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_registration_token(
        &self,
        bot_id: Uuid,
        token: &str,
    ) -> Result<(), RepositoryError> {
        let hashed_token = hash_registration_token(token);
        sqlx::query(
            r#"
            UPDATE bots
            SET registration_token = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(hashed_token)
        .bind(Utc::now())
        .bind(bot_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE bots
            SET status = 'destroyed', updated_at = $1
            WHERE id = $2
            "#,
        )
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn hard_delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            DELETE FROM bots
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn increment_bot_counter(
        &self,
        account_id: Uuid,
    ) -> Result<(bool, i32, i32), RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT success, current_count, max_count
            FROM increment_bot_counter($1)
            "#,
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                // Counter doesn't exist yet - query current state
                RepositoryError::NotFound(format!("Account counter for {}", account_id))
            }
            _ => RepositoryError::DatabaseError(e),
        })?;

        let success: bool = row.try_get("success")?;
        let current_count: i32 = row.try_get("current_count")?;
        let max_count: i32 = row.try_get("max_count")?;

        Ok((success, current_count, max_count))
    }

    async fn decrement_bot_counter(&self, account_id: Uuid) -> Result<(), RepositoryError> {
        sqlx::query("SELECT decrement_bot_counter($1)")
            .bind(account_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn list_stale_bots(
        &self,
        threshold: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<Bot>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, account_id, name, persona, status, droplet_id,
                   desired_config_version_id, applied_config_version_id,
                   registration_token, created_at, updated_at, last_heartbeat_at
            FROM bots
            WHERE status = 'online'
              AND (last_heartbeat_at < $1 OR last_heartbeat_at IS NULL)
            "#,
        )
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_bot).collect()
    }
}

// MED-007: Status and persona mapping now handled by strum derive macros
// BotStatus and Persona enums use #[derive(Display, EnumString)] for automatic
// String <-> Enum conversion with snake_case serialization.

fn row_to_bot(row: &sqlx::postgres::PgRow) -> Result<Bot, RepositoryError> {
    let status_str: String = row.try_get("status")?;
    let persona_str: String = row.try_get("persona")?;

    Ok(Bot {
        id: row.try_get("id")?,
        account_id: row.try_get("account_id")?,
        name: row.try_get("name")?,
        persona: Persona::from_str(&persona_str).map_err(|_| {
            RepositoryError::InvalidData(format!("Unknown persona: {}", persona_str))
        })?,
        status: BotStatus::from_str(&status_str)
            .map_err(|_| RepositoryError::InvalidData(format!("Unknown status: {}", status_str)))?,
        droplet_id: row.try_get("droplet_id")?,
        desired_config_version_id: row.try_get("desired_config_version_id")?,
        applied_config_version_id: row.try_get("applied_config_version_id")?,
        registration_token: row.try_get("registration_token")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        last_heartbeat_at: row.try_get("last_heartbeat_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::hash_registration_token;

    #[test]
    fn hash_registration_token_is_stable_and_prefixed() {
        let token = "reg-token-123";
        let hashed = hash_registration_token(token);
        let hashed_again = hash_registration_token(token);

        assert_eq!(hashed, hashed_again);
        assert!(hashed.starts_with("sha256:"));
        assert_ne!(hashed, token);
    }
}
