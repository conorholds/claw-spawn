use crate::domain::{Bot, BotStatus, StoredBotConfig};
use crate::infrastructure::{BotRepository, ConfigRepository, RepositoryError};
use chrono::{Duration, Utc};
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum LifecycleError {
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Bot not in valid state: {0:?}")]
    InvalidState(BotStatus),
    #[error("Config not found: {0}")]
    ConfigNotFound(Uuid),
    #[error("Config version conflict: acknowledging {acknowledged}, but desired is {desired:?}")]
    ConfigVersionConflict { acknowledged: Uuid, desired: Option<Uuid> },
}

pub struct BotLifecycleService<B, C>
where
    B: BotRepository,
    C: ConfigRepository,
{
    bot_repo: Arc<B>,
    config_repo: Arc<C>,
}

impl<B, C> BotLifecycleService<B, C>
where
    B: BotRepository,
    C: ConfigRepository,
{
    pub fn new(
        bot_repo: Arc<B>,
        config_repo: Arc<C>,
    ) -> Self {
        Self {
            bot_repo,
            config_repo,
        }
    }

    pub async fn get_bot(&self, bot_id: Uuid) -> Result<Bot, LifecycleError> {
        Ok(self.bot_repo.get_by_id(bot_id).await?)
    }

    pub async fn get_bot_with_token(&self, bot_id: Uuid, token: &str) -> Result<Bot, LifecycleError> {
        Ok(self.bot_repo.get_by_id_with_token(bot_id, token).await?)
    }

    /// PERF-002: List bots with pagination support
    /// - limit: Maximum number of bots to return
    /// - offset: Number of bots to skip
    pub async fn list_account_bots(
        &self,
        account_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Bot>, LifecycleError> {
        Ok(self.bot_repo.list_by_account_paginated(account_id, limit, offset).await?)
    }

    pub async fn create_bot_config(
        &self,
        bot_id: Uuid,
        config: StoredBotConfig,
    ) -> Result<StoredBotConfig, LifecycleError> {
        let bot = self.bot_repo.get_by_id(bot_id).await?;

        if bot.status == BotStatus::Destroyed {
            return Err(LifecycleError::InvalidState(bot.status));
        }

        // CRIT-007: Use atomic version generation to prevent race conditions
        let next_version = self.config_repo.get_next_version_atomic(bot_id).await?;

        let config_with_version = StoredBotConfig {
            id: Uuid::new_v4(),
            bot_id,
            version: next_version,
            created_at: chrono::Utc::now(),
            ..config
        };

        self.config_repo.create(&config_with_version).await?;
        self.bot_repo
            .update_config_version(bot_id, Some(config_with_version.id), bot.applied_config_version_id)
            .await?;

        info!(
            "Updated bot {} config to version {}",
            bot_id, config_with_version.version
        );

        Ok(config_with_version)
    }

    pub async fn acknowledge_config(
        &self,
        bot_id: Uuid,
        config_id: Uuid,
    ) -> Result<(), LifecycleError> {
        let config = self.config_repo.get_by_id(config_id).await?;

        if config.bot_id != bot_id {
            return Err(LifecycleError::ConfigNotFound(config_id));
        }

        // MED-004: Check for config version conflict
        let bot = self.bot_repo.get_by_id(bot_id).await?;
        if bot.desired_config_version_id != Some(config_id) {
            return Err(LifecycleError::ConfigVersionConflict {
                acknowledged: config_id,
                desired: bot.desired_config_version_id,
            });
        }

        self.bot_repo
            .update_config_version(bot_id, Some(config_id), Some(config_id))
            .await?;

        if bot.status == BotStatus::Provisioning || bot.status == BotStatus::Pending {
            self.bot_repo.update_status(bot_id, BotStatus::Online).await?;
        }

        info!("Bot {} acknowledged config {}", bot_id, config_id);
        Ok(())
    }

    pub async fn get_desired_config(&self, bot_id: Uuid) -> Result<Option<StoredBotConfig>, LifecycleError> {
        let bot = self.bot_repo.get_by_id(bot_id).await?;

        if let Some(config_id) = bot.desired_config_version_id {
            match self.config_repo.get_by_id(config_id).await {
                Ok(config) => Ok(Some(config)),
                Err(RepositoryError::NotFound(_)) => Ok(None),
                Err(e) => Err(e.into()),
            }
        } else {
            Ok(None)
        }
    }

    pub async fn record_heartbeat(&self, bot_id: Uuid) -> Result<(), LifecycleError> {
        self.bot_repo.update_heartbeat(bot_id).await?;
        Ok(())
    }

    /// Check for bots with stale heartbeats and mark them as Error (HIGH-001)
    pub async fn check_stale_bots(
        &self,
        heartbeat_timeout: Duration,
    ) -> Result<Vec<Bot>, LifecycleError> {
        let threshold = Utc::now() - heartbeat_timeout;
        let stale_bots = self.bot_repo.list_stale_bots(threshold).await?;

        for bot in &stale_bots {
            warn!(
                "Bot {} heartbeat timeout (last: {:?}), marking as Error",
                bot.id, bot.last_heartbeat_at
            );
            self.bot_repo.update_status(bot.id, BotStatus::Error).await?;
        }

        if !stale_bots.is_empty() {
            info!(
                "Marked {} bot(s) as Error due to heartbeat timeout",
                stale_bots.len()
            );
        }

        Ok(stale_bots)
    }
}
