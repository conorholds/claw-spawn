use crate::domain::{Bot, BotStatus, StoredBotConfig};
use crate::infrastructure::{BotRepository, ConfigRepository, RepositoryError};
use std::sync::Arc;
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum LifecycleError {
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Bot not in valid state: {0:?}")]
    InvalidState(BotStatus),
    #[error("Config not found: {0}")]
    ConfigNotFound(Uuid),
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

    pub async fn list_account_bots(&self, account_id: Uuid) -> Result<Vec<Bot>, LifecycleError> {
        Ok(self.bot_repo.list_by_account(account_id).await?)
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

        let config_with_version = StoredBotConfig {
            id: Uuid::new_v4(),
            bot_id,
            version: self.get_next_version(bot_id).await?,
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

    async fn get_next_version(&self, bot_id: Uuid) -> Result<i32, LifecycleError> {
        let configs = self.config_repo.list_by_bot(bot_id).await?;
        let max_version = configs.iter().map(|c| c.version).max().unwrap_or(0);
        Ok(max_version + 1)
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

        self.bot_repo
            .update_config_version(bot_id, Some(config_id), Some(config_id))
            .await?;

        if let Ok(bot) = self.bot_repo.get_by_id(bot_id).await {
            if bot.status == BotStatus::Provisioning || bot.status == BotStatus::Pending {
                self.bot_repo.update_status(bot_id, BotStatus::Online).await?;
            }
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
}
