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
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn, Span};
use uuid::Uuid;

/// MED-005: Maximum length for sanitized bot names
const MAX_BOT_NAME_LENGTH: usize = 64;

/// REL-001: Retry configuration for compensating transactions
const RETRY_ATTEMPTS: u32 = 3;
const RETRY_DELAYS_MS: [u64; 3] = [100, 200, 400];

/// REL-001: Retry an async operation with exponential backoff
/// Logs each retry attempt with structured context
async fn retry_with_backoff<F, Fut, T, E>(
    operation_name: &str,
    bot_id: Uuid,
    f: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    for (attempt, delay_ms) in RETRY_DELAYS_MS.iter().enumerate() {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let attempt_num = attempt + 1;
                warn!(
                    bot_id = %bot_id,
                    operation = %operation_name,
                    attempt = attempt_num,
                    max_attempts = RETRY_ATTEMPTS,
                    error = %e,
                    "Operation failed, will retry after {}ms", delay_ms
                );
                sleep(Duration::from_millis(*delay_ms)).await;
            }
        }
    }

    // Final attempt without delay
    match f().await {
        Ok(result) => Ok(result),
        Err(e) => {
            error!(
                bot_id = %bot_id,
                operation = %operation_name,
                attempts = RETRY_ATTEMPTS,
                error = %e,
                "All retry attempts exhausted"
            );
            Err(e)
        }
    }
}

/// MED-005: Sanitize user-provided bot name to prevent injection/truncation issues
/// - Removes/replaces special characters
/// - Limits length to prevent truncation issues
fn sanitize_bot_name(name: &str) -> String {
    // Replace problematic characters with safe alternatives
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            // Allow alphanumeric, spaces, hyphens, underscores
            'a'..='z' | 'A'..='Z' | '0'..='9' | ' ' | '-' | '_' => c,
            // Replace other special characters with underscore
            _ => '_',
        })
        .collect();

    // Trim leading/trailing whitespace and limit length
    let trimmed = sanitized.trim();
    if trimmed.len() > MAX_BOT_NAME_LENGTH {
        trimmed[..MAX_BOT_NAME_LENGTH].to_string()
    } else {
        trimmed.to_string()
    }
}

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
    control_plane_url: String,
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
        control_plane_url: String,
    ) -> Self {
        Self {
            do_client,
            account_repo,
            bot_repo,
            config_repo,
            droplet_repo,
            encryption,
            openclaw_image,
            control_plane_url,
        }
    }

    pub async fn create_bot(
        &self,
        account_id: Uuid,
        name: String,
        persona: Persona,
        config: BotConfig,
    ) -> Result<Bot, ProvisioningError> {
        // REL-003: Structured logging context
        let span = Span::current();
        span.record("account_id", account_id.to_string());

        let _account = self.account_repo.get_by_id(account_id).await?;
        
        // CRIT-002: Use atomic counter for race-condition-free limit checking
        let (success, _current_count, max_count) = self.bot_repo.increment_bot_counter(account_id).await?;
        
        if !success {
            warn!(
                account_id = %account_id,
                max_bots = max_count,
                "Account limit reached - cannot create bot"
            );
            return Err(ProvisioningError::AccountLimitReached(max_count));
        }

        // MED-005: Sanitize bot name before use
        let sanitized_name = sanitize_bot_name(&name);
        info!(
            account_id = %account_id,
            original_name = %name,
            sanitized_name = %sanitized_name,
            "Creating bot with sanitized name"
        );
        
        let mut bot = Bot::new(account_id, sanitized_name, persona);

        // CRIT-005: Resource cleanup - if DB operations fail after this point,
        // we need to decrement the counter we just incremented
        let result = self.create_bot_internal(&mut bot, config).await;
        
        if result.is_err() {
            // Decrement counter on failure to allow retry
            if let Err(e) = self.bot_repo.decrement_bot_counter(account_id).await {
                error!(
                    account_id = %account_id,
                    bot_id = %bot.id,
                    error = %e,
                    "Failed to decrement bot counter after failed creation"
                );
            }
        }
        
        result.map(|_| bot)
    }

    async fn create_bot_internal(
        &self,
        bot: &mut Bot,
        config: BotConfig,
    ) -> Result<(), ProvisioningError> {
        self.bot_repo.create(bot).await?;
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

        self.spawn_bot(bot, &config_with_encrypted).await?;

        Ok(())
    }

    async fn spawn_bot(&self, bot: &mut Bot, config: &StoredBotConfig) -> Result<(), ProvisioningError> {
        // REL-003: Add structured logging context
        let span = Span::current();
        span.record("bot_id", bot.id.to_string());
        span.record("account_id", bot.account_id.to_string());

        self.bot_repo
            .update_status(bot.id, BotStatus::Provisioning)
            .await?;
        bot.status = BotStatus::Provisioning;

        info!(
            bot_id = %bot.id,
            account_id = %bot.account_id,
            "Starting bot spawn process"
        );

        // MED-002: Safe string truncation instead of split
        let id_str = bot.id.to_string();
        let droplet_name = format!("openclaw-bot-{}", &id_str[..8.min(id_str.len())]);
        let registration_token = self.generate_registration_token(bot.id);
        
        // CRIT-001: Store registration token in database
        self.bot_repo.update_registration_token(bot.id, &registration_token).await?;
        bot.registration_token = Some(registration_token.clone());

        let user_data = self.generate_user_data(&registration_token, bot.id, config);

        let droplet_request = DropletCreateRequest {
            name: droplet_name,
            region: "nyc3".to_string(),
            size: "s-1vcpu-2gb".to_string(),
            image: self.openclaw_image.clone(),
            user_data,
            tags: vec!["openclaw".to_string(), format!("bot-{}", bot.id)],
        };

        // CRIT-005: Create droplet first, then attempt DB persistence with cleanup on failure
        let droplet = match self.do_client.create_droplet(droplet_request).await {
            Ok(d) => d,
            Err(DigitalOceanError::RateLimited) => {
                warn!(
                    bot_id = %bot.id,
                    "Rate limited by DigitalOcean, bot will retry"
                );
                self.bot_repo.update_status(bot.id, BotStatus::Pending).await?;
                bot.status = BotStatus::Pending;
                return Err(DigitalOceanError::RateLimited.into());
            }
            Err(e) => {
                error!(
                    bot_id = %bot.id,
                    error = %e,
                    "Failed to create droplet for bot"
                );
                self.bot_repo.update_status(bot.id, BotStatus::Error).await?;
                bot.status = BotStatus::Error;
                return Err(e.into());
            }
        };

        // CRIT-005: Attempt DB operations with compensating cleanup on failure
        let db_result: Result<(), ProvisioningError> = async {
            self.droplet_repo.create(&droplet).await?;
            self.droplet_repo
                .update_bot_assignment(droplet.id, Some(bot.id))
                .await?;
            self.bot_repo.update_droplet(bot.id, Some(droplet.id)).await?;
            Ok(())
        }.await;

        if let Err(ref e) = db_result {
            // CRIT-005: DB persistence failed - attempt to clean up DO droplet
            error!(
                bot_id = %bot.id,
                droplet_id = droplet.id,
                error = %e,
                "DB persistence failed after DO droplet created. Attempting cleanup"
            );
            
            match self.do_client.destroy_droplet(droplet.id).await {
                Ok(_) => {
                    info!(
                        bot_id = %bot.id,
                        droplet_id = droplet.id,
                        "Successfully cleaned up droplet after DB failure"
                    );
                }
                Err(cleanup_err) => {
                    error!(
                        bot_id = %bot.id,
                        droplet_id = droplet.id,
                        error = %cleanup_err,
                        "FAILED TO CLEANUP: Droplet may be orphaned"
                    );
                }
            }
            
            // Update bot status to error since droplet creation failed at persistence stage
            if let Err(status_err) = self.bot_repo.update_status(bot.id, BotStatus::Error).await {
                error!(
                    bot_id = %bot.id,
                    error = %status_err,
                    "Failed to update bot status to error"
                );
            }
            bot.status = BotStatus::Error;
            
            return Err(db_result.unwrap_err());
        }

        bot.droplet_id = Some(droplet.id);

        info!(
            bot_id = %bot.id,
            droplet_id = droplet.id,
            "Successfully spawned droplet for bot"
        );

        Ok(())
    }

    fn generate_user_data(&self, registration_token: &str, bot_id: Uuid, _config: &StoredBotConfig) -> String {
        // Read the bootstrap script and prepend environment variables
        let bootstrap_script = include_str!("../../scripts/openclaw-bootstrap.sh");
        
        // CRIT-006: Use configured control plane URL instead of hardcoded value
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
            self.control_plane_url,
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

        // REL-003: Add structured logging span with context
        let span = Span::current();
        span.record("bot_id", bot_id.to_string());
        span.record("account_id", bot.account_id.to_string());

        if let Some(droplet_id) = bot.droplet_id {
            span.record("droplet_id", droplet_id);
            
            match self.do_client.destroy_droplet(droplet_id).await {
                Ok(_) => {
                    info!(
                        bot_id = %bot_id,
                        droplet_id = droplet_id,
                        "Destroyed droplet for bot"
                    );
                    
                    // REL-001: Retry on failure for compensating transaction
                    if let Err(e) = retry_with_backoff(
                        "mark_destroyed",
                        bot_id,
                        || self.droplet_repo.mark_destroyed(droplet_id)
                    ).await {
                        error!(
                            bot_id = %bot_id,
                            droplet_id = droplet_id,
                            error = %e,
                            "Failed to mark droplet as destroyed after retries"
                        );
                        return Err(e.into());
                    }
                }
                Err(DigitalOceanError::NotFound(_)) => {
                    warn!(
                        bot_id = %bot_id,
                        droplet_id = droplet_id,
                        "Droplet already destroyed or not found"
                    );
                    
                    // REL-001: Retry on failure for compensating transaction
                    if let Err(e) = retry_with_backoff(
                        "mark_destroyed",
                        bot_id,
                        || self.droplet_repo.mark_destroyed(droplet_id)
                    ).await {
                        error!(
                            bot_id = %bot_id,
                            droplet_id = droplet_id,
                            error = %e,
                            "Failed to mark droplet as destroyed after retries"
                        );
                        return Err(e.into());
                    }
                }
                Err(e) => {
                    error!(
                        bot_id = %bot_id,
                        droplet_id = droplet_id,
                        error = %e,
                        "Failed to destroy droplet"
                    );
                    return Err(e.into());
                }
            }
        }

        // REL-001: Retry DB updates with backoff
        if let Err(e) = retry_with_backoff(
            "update_droplet",
            bot_id,
            || self.bot_repo.update_droplet(bot_id, None)
        ).await {
            error!(
                bot_id = %bot_id,
                error = %e,
                "Failed to update bot droplet reference after retries"
            );
            return Err(e.into());
        }

        if let Err(e) = retry_with_backoff(
            "delete_bot",
            bot_id,
            || self.bot_repo.delete(bot_id)
        ).await {
            error!(
                bot_id = %bot_id,
                error = %e,
                "Failed to delete bot after retries"
            );
            return Err(e.into());
        }
        
        // CRIT-002: Decrement bot counter when bot is destroyed
        // REL-001: Retry counter decrement
        if let Err(e) = retry_with_backoff(
            "decrement_bot_counter",
            bot_id,
            || self.bot_repo.decrement_bot_counter(bot.account_id)
        ).await {
            error!(
                bot_id = %bot_id,
                account_id = %bot.account_id,
                error = %e,
                "Failed to decrement bot counter after retries - counter may be inconsistent"
            );
        }

        info!(
            bot_id = %bot_id,
            account_id = %bot.account_id,
            "Successfully destroyed bot"
        );
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

        if bot.status != BotStatus::Paused {
            return Err(ProvisioningError::InvalidConfig(
                format!("Bot {} is not in paused state (current: {:?})", bot_id, bot.status)
            ));
        }

        if let Some(droplet_id) = bot.droplet_id {
            // HIGH-002: Check droplet status before attempting reboot
            match self.do_client.get_droplet(droplet_id).await {
                Ok(droplet) => {
                    match droplet.status {
                        crate::domain::DropletStatus::Off => {
                            // Droplet is off, safe to reboot
                            self.do_client.reboot_droplet(droplet_id).await?;
                            info!("Resumed droplet {} for bot {}", droplet_id, bot_id);
                        }
                        crate::domain::DropletStatus::Active => {
                            // Droplet is already running, just update status
                            info!("Droplet {} for bot {} is already active", droplet_id, bot_id);
                        }
                        crate::domain::DropletStatus::New => {
                            // Droplet is still being created, not ready
                            return Err(ProvisioningError::InvalidConfig(
                                format!("Droplet {} is still being created, cannot resume yet", droplet_id)
                            ));
                        }
                        _ => {
                            return Err(ProvisioningError::InvalidConfig(
                                format!("Droplet {} is in state {:?}, cannot resume", droplet_id, droplet.status)
                            ));
                        }
                    }
                }
                Err(DigitalOceanError::NotFound(_)) => {
                    return Err(ProvisioningError::InvalidConfig(
                        format!("Droplet {} for bot {} no longer exists in DigitalOcean", droplet_id, bot_id)
                    ));
                }
                Err(e) => return Err(e.into()),
            }
        } else {
            return Err(ProvisioningError::InvalidConfig(
                format!("Bot {} has no associated droplet", bot_id)
            ));
        }

        self.bot_repo.update_status(bot_id, BotStatus::Online).await?;
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
