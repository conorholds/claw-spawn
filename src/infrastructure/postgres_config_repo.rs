use async_trait::async_trait;
use crate::domain::{EncryptedBotSecrets, RiskConfig, StoredBotConfig, TradingConfig};
use crate::infrastructure::{ConfigRepository, RepositoryError};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct PostgresConfigRepository {
    pool: PgPool,
}

impl PostgresConfigRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ConfigRepository for PostgresConfigRepository {
    async fn create(&self, config: &StoredBotConfig) -> Result<(), RepositoryError> {
        let trading_json = serde_json::to_value(&config.trading_config).map_err(|e| {
            RepositoryError::InvalidData(format!("Failed to serialize trading config: {}", e))
        })?;
        let risk_json = serde_json::to_value(&config.risk_config).map_err(|e| {
            RepositoryError::InvalidData(format!("Failed to serialize risk config: {}", e))
        })?;

        sqlx::query(
            r#"
            INSERT INTO bot_configs (id, bot_id, version, trading_config, risk_config, secrets_encrypted, llm_provider, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(config.id)
        .bind(config.bot_id)
        .bind(config.version)
        .bind(trading_json)
        .bind(risk_json)
        .bind(&config.secrets.llm_api_key_encrypted)
        .bind(&config.secrets.llm_provider)
        .bind(config.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<StoredBotConfig, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, bot_id, version, trading_config, risk_config, secrets_encrypted, llm_provider, created_at
            FROM bot_configs
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => RepositoryError::NotFound(format!("Config {}", id)),
            _ => RepositoryError::DatabaseError(e),
        })?;

        Ok(row_to_config(&row)?)
    }

    async fn get_latest_for_bot(
        &self,
        bot_id: Uuid,
    ) -> Result<Option<StoredBotConfig>, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, bot_id, version, trading_config, risk_config, secrets_encrypted, llm_provider, created_at
            FROM bot_configs
            WHERE bot_id = $1
            ORDER BY version DESC
            LIMIT 1
            "#,
        )
        .bind(bot_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_config(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_by_bot(&self, bot_id: Uuid) -> Result<Vec<StoredBotConfig>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, bot_id, version, trading_config, risk_config, secrets_encrypted, llm_provider, created_at
            FROM bot_configs
            WHERE bot_id = $1
            ORDER BY version ASC
            "#,
        )
        .bind(bot_id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_config).collect()
    }

    async fn get_next_version_atomic(&self, bot_id: Uuid) -> Result<i32, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT get_next_config_version_atomic($1) as version
            "#,
        )
        .bind(bot_id)
        .fetch_one(&self.pool)
        .await?;

        let version: i32 = row.try_get("version")?;
        Ok(version)
    }
}

fn row_to_config(row: &sqlx::postgres::PgRow) -> Result<StoredBotConfig, RepositoryError> {
    let trading_json: serde_json::Value = row.try_get("trading_config")?;
    let risk_json: serde_json::Value = row.try_get("risk_config")?;
    let encrypted_secrets: Vec<u8> = row.try_get("secrets_encrypted")?;

    let trading_config: TradingConfig = serde_json::from_value(trading_json).map_err(|e| {
        RepositoryError::InvalidData(format!("Failed to deserialize trading config: {}", e))
    })?;
    let risk_config: RiskConfig = serde_json::from_value(risk_json).map_err(|e| {
        RepositoryError::InvalidData(format!("Failed to deserialize risk config: {}", e))
    })?;

    let llm_provider: String = row.try_get("llm_provider")?;

    Ok(StoredBotConfig {
        id: row.try_get("id")?,
        bot_id: row.try_get("bot_id")?,
        version: row.try_get("version")?,
        trading_config,
        risk_config,
        secrets: EncryptedBotSecrets {
            llm_provider,
            llm_api_key_encrypted: encrypted_secrets,
        },
        created_at: row.try_get("created_at")?,
    })
}
