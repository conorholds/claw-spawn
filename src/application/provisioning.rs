use crate::domain::{
    Bot, BotConfig, BotStatus, DropletCreateRequest, EncryptedBotSecrets, Persona, StoredBotConfig,
};
use crate::infrastructure::{
    AccountRepository, BotRepository, ConfigRepository, DigitalOceanClient, DigitalOceanError,
    DropletRepository, RepositoryError, SecretsEncryption,
};
use rand::RngCore;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum ProvisioningError {
    #[error("DigitalOcean error: {0}")]
    DigitalOcean(#[from] DigitalOceanError),
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Account limit reached: max {0} bots allowed")]
    AccountLimitReached(i32),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Encryption error: {0}")]
    Encryption(String),
}

pub struct ProvisioningService<A, B, C, D>
where
    A: AccountRepository,
    B: BotRepository,
    C: ConfigRepository,
    D: DropletRepository,
{
    do_client: Arc<DigitalOceanClient>,
    account_repo: Arc<A>,
    bot_repo: Arc<B>,
    config_repo: Arc<C>,
    droplet_repo: Arc<D>,
    encryption: Arc<SecretsEncryption>,
    openclaw_image: String,
}

impl<A, B, C, D> ProvisioningService<A, B, C, D>
where
    A: AccountRepository,
    B: BotRepository,
    C: ConfigRepository,
    D: DropletRepository,
{
    pub fn new(
        do_client: Arc<DigitalOceanClient>,
        account_repo: Arc<A>,
        bot_repo: Arc<B>,
        config_repo: Arc<C>,
        droplet_repo: Arc<D>,
        encryption: Arc<SecretsEncryption>,
        openclaw_image: String,
    ) -> Self {
        Self {
            do_client,
            account_repo,
            bot_repo,
            config_repo,
            droplet_repo,
            encryption,
            openclaw_image,
        }
    }

    pub async fn create_bot(
        &self,
        account_id: Uuid,
        name: String,
        persona: Persona,
        config: BotConfig,
    ) -> Result<Bot, ProvisioningError> {
        let account = self.account_repo.get_by_id(account_id).await?;
        let existing_bots = self.bot_repo.list_by_account(account_id).await?;

        let active_count = existing_bots
            .iter()
            .filter(|b| b.status != BotStatus::Destroyed)
            .count() as i32;

        if active_count >= account.max_bots {
            return Err(ProvisioningError::AccountLimitReached(account.max_bots));
        }

        let mut bot = Bot::new(account_id, name, persona);

        self.bot_repo.create(&bot).await?;
        info!("Created bot record: {}", bot.id);

        let encrypted_key = self
            .encryption
            .encrypt(&config.secrets.llm_api_key)
            .map_err(|e| ProvisioningError::Encryption(e.to_string()))?;

        let config_id = Uuid::new_v4();
        let config_with_encrypted = StoredBotConfig {
            id: config_id,
            bot_id: bot.id,
            version: 1,
            trading_config: config.trading_config,
            risk_config: config.risk_config,
            secrets: EncryptedBotSecrets {
                llm_provider: config.secrets.llm_provider,
                llm_api_key_encrypted: encrypted_key,
            },
            created_at: chrono::Utc::now(),
        };

        self.config_repo.create(&config_with_encrypted).await?;
        info!("Created bot config version: {}", config_with_encrypted.id);

        self.bot_repo
            .update_config_version(bot.id, Some(config_with_encrypted.id), None)
            .await?;
        bot.desired_config_version_id = Some(config_with_encrypted.id);

        self.spawn_bot(&mut bot, &config_with_encrypted).await?;

        Ok(bot)
    }

    async fn spawn_bot(&self, bot: &mut Bot, config: &StoredBotConfig) -> Result<(), ProvisioningError> {
        self.bot_repo
            .update_status(bot.id, BotStatus::Provisioning)
            .await?;
        bot.status = BotStatus::Provisioning;

        // MED-002: Safe string truncation instead of split
        let id_str = bot.id.to_string();
        let droplet_name = format!("openclaw-bot-{}", &id_str[..8.min(id_str.len())]);
        let registration_token = self.generate_registration_token(bot.id);

        let user_data = self.generate_user_data(&registration_token, bot.id, config);

        let droplet_request = DropletCreateRequest {
            name: droplet_name,
            region: "nyc3".to_string(),
            size: "s-1vcpu-2gb".to_string(),
            image: self.openclaw_image.clone(),
            user_data,
            tags: vec!["openclaw".to_string(), format!("bot-{}", bot.id)],
        };

        let droplet = match self.do_client.create_droplet(droplet_request).await {
            Ok(d) => d,
            Err(DigitalOceanError::RateLimited) => {
                warn!("Rate limited by DigitalOcean, bot {} will retry", bot.id);
                self.bot_repo.update_status(bot.id, BotStatus::Pending).await?;
                bot.status = BotStatus::Pending;
                return Err(DigitalOceanError::RateLimited.into());
            }
            Err(e) => {
                error!("Failed to create droplet for bot {}: {}", bot.id, e);
                self.bot_repo.update_status(bot.id, BotStatus::Error).await?;
                bot.status = BotStatus::Error;
                return Err(e.into());
            }
        };

        self.droplet_repo.create(&droplet).await?;
        self.droplet_repo
            .update_bot_assignment(droplet.id, Some(bot.id))
            .await?;
        self.bot_repo.update_droplet(bot.id, Some(droplet.id)).await?;

        bot.droplet_id = Some(droplet.id);

        info!(
            "Successfully spawned droplet {} for bot {}",
            droplet.id, bot.id
        );

        Ok(())
    }

    fn generate_user_data(&self, registration_token: &str, bot_id: Uuid, _config: &StoredBotConfig) -> String {
        // Read the bootstrap script and prepend environment variables
        let bootstrap_script = include_str!("../../scripts/openclaw-bootstrap.sh");
        
        format!(
            r##"#!/bin/bash
# OpenClaw Bot Bootstrap for Bot {}
set -e
set -x

export REGISTRATION_TOKEN="{}"
export BOT_ID="{}"
export CONTROL_PLANE_URL="{}"

# Start of embedded bootstrap script
{}
"##,
            bot_id,
            registration_token,
            bot_id,
            "https://api.cedros.io",
            bootstrap_script
        )
    }

    fn generate_registration_token(&self, _bot_id: Uuid) -> String {
        let mut token = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token);
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &token)
    }

    pub async fn destroy_bot(&self, bot_id: Uuid) -> Result<(), ProvisioningError> {
        let bot = self.bot_repo.get_by_id(bot_id).await?;

        if let Some(droplet_id) = bot.droplet_id {
            match self.do_client.destroy_droplet(droplet_id).await {
                Ok(_) => {
                    info!("Destroyed droplet {} for bot {}", droplet_id, bot_id);
                    self.droplet_repo.mark_destroyed(droplet_id).await?;
                }
                Err(DigitalOceanError::NotFound(_)) => {
                    warn!("Droplet {} already destroyed or not found", droplet_id);
                    self.droplet_repo.mark_destroyed(droplet_id).await?;
                }
                Err(e) => return Err(e.into()),
            }
        }

        self.bot_repo.update_droplet(bot_id, None).await?;
        self.bot_repo.delete(bot_id).await?;

        info!("Successfully destroyed bot {}", bot_id);
        Ok(())
    }

    pub async fn pause_bot(&self, bot_id: Uuid) -> Result<(), ProvisioningError> {
        let bot = self.bot_repo.get_by_id(bot_id).await?;

        if let Some(droplet_id) = bot.droplet_id {
            self.do_client.shutdown_droplet(droplet_id).await?;
            info!("Paused droplet {} for bot {}", droplet_id, bot_id);
        }

        self.bot_repo.update_status(bot_id, BotStatus::Paused).await?;
        Ok(())
    }

    pub async fn resume_bot(&self, bot_id: Uuid) -> Result<(), ProvisioningError> {
        let bot = self.bot_repo.get_by_id(bot_id).await?;

        if bot.status == BotStatus::Paused {
            if let Some(droplet_id) = bot.droplet_id {
                self.do_client.reboot_droplet(droplet_id).await?;
                info!("Resumed droplet {} for bot {}", droplet_id, bot_id);
            }

            self.bot_repo.update_status(bot_id, BotStatus::Online).await?;
        }

        Ok(())
    }

    pub async fn redeploy_bot(&self, bot_id: Uuid) -> Result<(), ProvisioningError> {
        let mut bot = self.bot_repo.get_by_id(bot_id).await?;

        if let Some(droplet_id) = bot.droplet_id {
            match self.do_client.destroy_droplet(droplet_id).await {
                Ok(_) | Err(DigitalOceanError::NotFound(_)) => {
                    self.droplet_repo.mark_destroyed(droplet_id).await?;
                }
                Err(e) => return Err(e.into()),
            }
        }

        // Get the latest config for redeployment
        let config = self.config_repo
            .get_latest_for_bot(bot_id)
            .await?
            .ok_or_else(|| ProvisioningError::InvalidConfig("No config found for redeployment".to_string()))?;

        bot.droplet_id = None;
        self.spawn_bot(&mut bot, &config).await?;

        info!("Successfully redeployed bot {}", bot_id);
        Ok(())
    }

    pub async fn sync_droplet_status(&self, bot_id: Uuid) -> Result<(), ProvisioningError> {
        let bot = self.bot_repo.get_by_id(bot_id).await?;

        if let Some(droplet_id) = bot.droplet_id {
            match self.do_client.get_droplet(droplet_id).await {
                Ok(droplet) => {
                    let status_str = match droplet.status {
                        crate::domain::DropletStatus::Active => "active",
                        crate::domain::DropletStatus::New => "new",
                        crate::domain::DropletStatus::Off => "off",
                        _ => "error",
                    };

                    self.droplet_repo.update_status(droplet_id, status_str).await?;

                    if let Some(ip) = droplet.ip_address {
                        self.droplet_repo.update_ip(droplet_id, Some(ip)).await?;
                    }

                    if bot.status == BotStatus::Provisioning
                        && droplet.status == crate::domain::DropletStatus::Active
                    {
                        info!(
                            "Bot {} droplet {} is now active, waiting for heartbeat",
                            bot_id, droplet_id
                        );
                    }
                }
                Err(DigitalOceanError::NotFound(_)) => {
                    warn!("Droplet {} for bot {} not found", droplet_id, bot_id);
                    if bot.status != BotStatus::Destroyed && bot.status != BotStatus::Error {
                        self.bot_repo.update_status(bot_id, BotStatus::Error).await?;
                    }
                }
                Err(e) => {
                    warn!("Failed to sync droplet {} status: {}", droplet_id, e);
                }
            }
        }

        Ok(())
    }
}
