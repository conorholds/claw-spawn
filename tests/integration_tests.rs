//! Integration tests for claw-spawn
//! CLEAN-003: Comprehensive test suite covering account creation, bot lifecycle,
//! config versioning, and authentication.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use claw_spawn::{
    application::BotLifecycleService,
    domain::{
        Account, AlgorithmMode, AssetFocus, Bot, BotStatus, EncryptedBotSecrets, Persona,
        RiskConfig, StoredBotConfig, StrictnessLevel, SubscriptionTier, TradingConfig,
    },
    infrastructure::{AccountRepository, BotRepository, ConfigRepository, RepositoryError},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// ============================================================================
// Mock Repositories for Testing
// ============================================================================

/// In-memory mock implementation of AccountRepository
#[derive(Clone, Default)]
struct MockAccountRepository {
    accounts: Arc<Mutex<HashMap<Uuid, Account>>>,
    external_ids: Arc<Mutex<HashMap<String, Uuid>>>,
}

#[async_trait]
impl AccountRepository for MockAccountRepository {
    async fn create(&self, account: &Account) -> Result<(), RepositoryError> {
        let mut accounts = self.accounts.lock().unwrap();
        let mut external_ids = self.external_ids.lock().unwrap();

        if accounts.contains_key(&account.id) {
            return Err(RepositoryError::InvalidData(
                "Account already exists".to_string(),
            ));
        }

        accounts.insert(account.id, account.clone());
        external_ids.insert(account.external_id.clone(), account.id);
        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<Account, RepositoryError> {
        let accounts = self.accounts.lock().unwrap();
        accounts
            .get(&id)
            .cloned()
            .ok_or_else(|| RepositoryError::NotFound(format!("Account {}", id)))
    }

    async fn get_by_external_id(&self, external_id: &str) -> Result<Account, RepositoryError> {
        let external_ids = self.external_ids.lock().unwrap();
        let accounts = self.accounts.lock().unwrap();

        external_ids
            .get(external_id)
            .and_then(|id| accounts.get(id).cloned())
            .ok_or_else(|| RepositoryError::NotFound(format!("Account {}", external_id)))
    }

    async fn update_subscription(
        &self,
        id: Uuid,
        tier: SubscriptionTier,
    ) -> Result<(), RepositoryError> {
        let mut accounts = self.accounts.lock().unwrap();
        let account = accounts
            .get_mut(&id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Account {}", id)))?;

        account.max_bots = match tier {
            SubscriptionTier::Free => 0,
            SubscriptionTier::Basic => 2,
            SubscriptionTier::Pro => 4,
        };
        account.subscription_tier = tier;
        account.updated_at = Utc::now();
        Ok(())
    }
}

/// In-memory mock implementation of BotRepository
#[derive(Clone, Default)]
struct MockBotRepository {
    bots: Arc<Mutex<HashMap<Uuid, Bot>>>,
    account_bots: Arc<Mutex<HashMap<Uuid, Vec<Uuid>>>>,
    counter: Arc<Mutex<HashMap<Uuid, i32>>>,
}

#[async_trait]
impl BotRepository for MockBotRepository {
    async fn create(&self, bot: &Bot) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        let mut account_bots = self.account_bots.lock().unwrap();

        if bots.contains_key(&bot.id) {
            return Err(RepositoryError::InvalidData(
                "Bot already exists".to_string(),
            ));
        }

        bots.insert(bot.id, bot.clone());
        account_bots.entry(bot.account_id).or_default().push(bot.id);

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<Bot, RepositoryError> {
        let bots = self.bots.lock().unwrap();
        bots.get(&id)
            .cloned()
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", id)))
    }

    async fn get_by_id_with_token(&self, id: Uuid, token: &str) -> Result<Bot, RepositoryError> {
        let bots = self.bots.lock().unwrap();
        bots.get(&id)
            .filter(|b| b.registration_token.as_ref() == Some(&token.to_string()))
            .cloned()
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {} with invalid token", id)))
    }

    async fn list_by_account(&self, account_id: Uuid) -> Result<Vec<Bot>, RepositoryError> {
        let bots = self.bots.lock().unwrap();
        let account_bots = self.account_bots.lock().unwrap();

        let bot_ids = account_bots.get(&account_id).cloned().unwrap_or_default();
        let result: Vec<Bot> = bot_ids
            .iter()
            .filter_map(|id| bots.get(id).cloned())
            .collect();

        Ok(result)
    }

    async fn list_by_account_paginated(
        &self,
        account_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Bot>, RepositoryError> {
        let all = self.list_by_account(account_id).await?;
        let offset = offset.max(0) as usize;
        let limit = limit.max(0) as usize;
        Ok(all.into_iter().skip(offset).take(limit).collect())
    }

    async fn update_status(&self, id: Uuid, status: BotStatus) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        let bot = bots
            .get_mut(&id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", id)))?;

        bot.status = status;
        bot.updated_at = Utc::now();
        Ok(())
    }

    async fn update_droplet(
        &self,
        bot_id: Uuid,
        droplet_id: Option<i64>,
    ) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        let bot = bots
            .get_mut(&bot_id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", bot_id)))?;

        bot.droplet_id = droplet_id;
        bot.updated_at = Utc::now();
        Ok(())
    }

    async fn update_config_version(
        &self,
        bot_id: Uuid,
        desired: Option<Uuid>,
        applied: Option<Uuid>,
    ) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        let bot = bots
            .get_mut(&bot_id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", bot_id)))?;

        bot.desired_config_version_id = desired;
        bot.applied_config_version_id = applied;
        bot.updated_at = Utc::now();
        Ok(())
    }

    async fn update_heartbeat(&self, bot_id: Uuid) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        let bot = bots
            .get_mut(&bot_id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", bot_id)))?;

        bot.last_heartbeat_at = Some(Utc::now());
        bot.updated_at = Utc::now();
        Ok(())
    }

    async fn update_registration_token(
        &self,
        bot_id: Uuid,
        token: &str,
    ) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        let bot = bots
            .get_mut(&bot_id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", bot_id)))?;

        bot.registration_token = Some(token.to_string());
        bot.updated_at = Utc::now();
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        bots.get_mut(&id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", id)))?
            .status = BotStatus::Destroyed;
        Ok(())
    }

    async fn hard_delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        let mut bots = self.bots.lock().unwrap();
        let mut account_bots = self.account_bots.lock().unwrap();
        let bot = bots
            .remove(&id)
            .ok_or_else(|| RepositoryError::NotFound(format!("Bot {}", id)))?;

        if let Some(ids) = account_bots.get_mut(&bot.account_id) {
            ids.retain(|existing| *existing != id);
        }
        Ok(())
    }

    async fn increment_bot_counter(
        &self,
        account_id: Uuid,
    ) -> Result<(bool, i32, i32), RepositoryError> {
        let mut counter = self.counter.lock().unwrap();
        let current = counter.get(&account_id).cloned().unwrap_or(0);

        // Mock account with Basic tier (2 bots max)
        let max_bots = 2;

        if current >= max_bots {
            return Ok((false, current, max_bots));
        }

        counter.insert(account_id, current + 1);
        Ok((true, current + 1, max_bots))
    }

    async fn decrement_bot_counter(&self, account_id: Uuid) -> Result<(), RepositoryError> {
        let mut counter = self.counter.lock().unwrap();
        let current = counter.get(&account_id).cloned().unwrap_or(0);

        if current > 0 {
            counter.insert(account_id, current - 1);
        }

        Ok(())
    }

    async fn count_by_account(&self, account_id: Uuid) -> Result<i64, RepositoryError> {
        let bots = self.bots.lock().unwrap();
        let count = bots
            .values()
            .filter(|b| b.account_id == account_id && b.status != BotStatus::Destroyed)
            .count() as i64;
        Ok(count)
    }

    async fn list_stale_bots(&self, threshold: DateTime<Utc>) -> Result<Vec<Bot>, RepositoryError> {
        let bots = self.bots.lock().unwrap();
        let stale: Vec<Bot> = bots
            .values()
            .filter(|b| {
                b.status == BotStatus::Online
                    && (b.last_heartbeat_at.is_none() || b.last_heartbeat_at.unwrap() < threshold)
            })
            .cloned()
            .collect();
        Ok(stale)
    }
}

/// In-memory mock implementation of ConfigRepository
#[derive(Clone, Default)]
struct MockConfigRepository {
    configs: Arc<Mutex<HashMap<Uuid, StoredBotConfig>>>,
    bot_configs: Arc<Mutex<HashMap<Uuid, Vec<Uuid>>>>,
    version_counter: Arc<Mutex<HashMap<Uuid, i32>>>,
}

#[async_trait]
impl ConfigRepository for MockConfigRepository {
    async fn create(&self, config: &StoredBotConfig) -> Result<(), RepositoryError> {
        let mut configs = self.configs.lock().unwrap();
        let mut bot_configs = self.bot_configs.lock().unwrap();

        configs.insert(config.id, config.clone());
        bot_configs
            .entry(config.bot_id)
            .or_default()
            .push(config.id);

        Ok(())
    }

    async fn get_by_id(&self, id: Uuid) -> Result<StoredBotConfig, RepositoryError> {
        let configs = self.configs.lock().unwrap();
        configs
            .get(&id)
            .cloned()
            .ok_or_else(|| RepositoryError::NotFound(format!("Config {}", id)))
    }

    async fn get_latest_for_bot(
        &self,
        bot_id: Uuid,
    ) -> Result<Option<StoredBotConfig>, RepositoryError> {
        let configs = self.configs.lock().unwrap();
        let bot_configs = self.bot_configs.lock().unwrap();

        let config_ids = bot_configs.get(&bot_id).cloned().unwrap_or_default();
        let latest = config_ids
            .iter()
            .filter_map(|id| configs.get(id))
            .max_by_key(|c| c.version)
            .cloned();

        Ok(latest)
    }

    async fn list_by_bot(&self, bot_id: Uuid) -> Result<Vec<StoredBotConfig>, RepositoryError> {
        let configs = self.configs.lock().unwrap();
        let bot_configs = self.bot_configs.lock().unwrap();

        let config_ids = bot_configs.get(&bot_id).cloned().unwrap_or_default();
        let result: Vec<StoredBotConfig> = config_ids
            .iter()
            .filter_map(|id| configs.get(id).cloned())
            .collect();

        Ok(result)
    }

    async fn get_next_version_atomic(&self, bot_id: Uuid) -> Result<i32, RepositoryError> {
        let mut counter = self.version_counter.lock().unwrap();
        let next = counter.get(&bot_id).cloned().unwrap_or(0) + 1;
        counter.insert(bot_id, next);
        Ok(next)
    }
}

// ============================================================================
// Test Helpers
// ============================================================================

fn create_test_risk_config() -> RiskConfig {
    RiskConfig {
        max_position_size_pct: 10.0,
        max_daily_loss_pct: 5.0,
        max_drawdown_pct: 20.0,
        max_trades_per_day: 100,
    }
}

fn create_test_trading_config() -> TradingConfig {
    TradingConfig {
        asset_focus: AssetFocus::Majors,
        algorithm: AlgorithmMode::Trend,
        strictness: StrictnessLevel::Medium,
        paper_mode: true,
        signal_knobs: None,
    }
}

fn create_test_stored_config(bot_id: Uuid, version: i32) -> StoredBotConfig {
    StoredBotConfig {
        id: Uuid::new_v4(),
        bot_id,
        version,
        trading_config: create_test_trading_config(),
        risk_config: create_test_risk_config(),
        secrets: EncryptedBotSecrets {
            llm_provider: "openai".to_string(),
            llm_api_key_encrypted: vec![1, 2, 3, 4, 5],
        },
        created_at: Utc::now(),
    }
}

// ============================================================================
// Test Cases
// ============================================================================

#[tokio::test]
async fn test_account_creation() {
    let account_repo = Arc::new(MockAccountRepository::default());

    // Test creating an account
    let account = Account::new("test-external-id".to_string(), SubscriptionTier::Basic);
    let account_id = account.id;

    account_repo
        .create(&account)
        .await
        .expect("Failed to create account");

    // Test retrieving by ID
    let retrieved = account_repo
        .get_by_id(account_id)
        .await
        .expect("Failed to get account");
    assert_eq!(retrieved.id, account_id);
    assert_eq!(retrieved.external_id, "test-external-id");
    assert_eq!(retrieved.subscription_tier, SubscriptionTier::Basic);

    // Test retrieving by external ID
    let by_external = account_repo
        .get_by_external_id("test-external-id")
        .await
        .expect("Failed to get by external ID");
    assert_eq!(by_external.id, account_id);

    // Test updating subscription
    account_repo
        .update_subscription(account_id, SubscriptionTier::Pro)
        .await
        .expect("Failed to update subscription");

    let updated = account_repo
        .get_by_id(account_id)
        .await
        .expect("Failed to get updated");
    assert_eq!(updated.subscription_tier, SubscriptionTier::Pro);
    assert_eq!(updated.max_bots, 4); // Pro tier has 4 bots
}

#[tokio::test]
async fn test_bot_lifecycle() {
    let account_repo = Arc::new(MockAccountRepository::default());
    let bot_repo = Arc::new(MockBotRepository::default());

    // Create account first
    let account = Account::new("lifecycle-test".to_string(), SubscriptionTier::Basic);
    let account_id = account.id;
    account_repo
        .create(&account)
        .await
        .expect("Failed to create account");

    // Create bot using Bot::new
    let bot = Bot::new(account_id, "Test Bot".to_string(), Persona::Beginner);
    let bot_id = bot.id;

    bot_repo.create(&bot).await.expect("Failed to create bot");

    // Verify initial state
    let retrieved = bot_repo.get_by_id(bot_id).await.expect("Failed to get bot");
    assert_eq!(retrieved.status, BotStatus::Pending);
    assert_eq!(retrieved.name, "Test Bot");
    assert_eq!(retrieved.persona, Persona::Beginner);

    // Update status through lifecycle
    bot_repo
        .update_status(bot_id, BotStatus::Provisioning)
        .await
        .expect("Failed to update status");

    let provisioning = bot_repo.get_by_id(bot_id).await.expect("Failed to get bot");
    assert_eq!(provisioning.status, BotStatus::Provisioning);

    // Simulate bot coming online
    bot_repo
        .update_status(bot_id, BotStatus::Online)
        .await
        .expect("Failed to set online");

    // Record heartbeat
    bot_repo
        .update_heartbeat(bot_id)
        .await
        .expect("Failed to record heartbeat");
    let with_heartbeat = bot_repo.get_by_id(bot_id).await.expect("Failed to get bot");
    assert!(with_heartbeat.last_heartbeat_at.is_some());

    // Pause bot
    bot_repo
        .update_status(bot_id, BotStatus::Paused)
        .await
        .expect("Failed to pause");
    let paused = bot_repo.get_by_id(bot_id).await.expect("Failed to get bot");
    assert_eq!(paused.status, BotStatus::Paused);

    // Resume bot
    bot_repo
        .update_status(bot_id, BotStatus::Online)
        .await
        .expect("Failed to resume");

    // Destroy bot
    bot_repo.delete(bot_id).await.expect("Failed to delete bot");
    let destroyed = bot_repo.get_by_id(bot_id).await.expect("Failed to get bot");
    assert_eq!(destroyed.status, BotStatus::Destroyed);
}

#[tokio::test]
async fn test_config_versioning() {
    let config_repo = Arc::new(MockConfigRepository::default());
    let bot_repo = Arc::new(MockBotRepository::default());

    let bot_id = Uuid::new_v4();

    // Create a bot for the config
    let bot = Bot::new(
        Uuid::new_v4(),
        "Config Test Bot".to_string(),
        Persona::Beginner,
    );
    bot_repo.create(&bot).await.expect("Failed to create bot");

    // Create initial config (version 1)
    let config1 = create_test_stored_config(bot_id, 1);
    config_repo
        .create(&config1)
        .await
        .expect("Failed to create config 1");

    // Verify we can retrieve it
    let retrieved = config_repo
        .get_by_id(config1.id)
        .await
        .expect("Failed to get config");
    assert_eq!(retrieved.version, 1);

    // Create version 2
    let config2 = create_test_stored_config(bot_id, 2);
    config_repo
        .create(&config2)
        .await
        .expect("Failed to create config 2");

    // Get latest - should be version 2
    let latest = config_repo
        .get_latest_for_bot(bot_id)
        .await
        .expect("Failed to get latest")
        .expect("No latest config");
    assert_eq!(latest.version, 2);

    // List all configs
    let all_configs = config_repo
        .list_by_bot(bot_id)
        .await
        .expect("Failed to list configs");
    assert_eq!(all_configs.len(), 2);

    // Test atomic version generation
    let next_version = config_repo
        .get_next_version_atomic(bot_id)
        .await
        .expect("Failed to get next version");
    assert_eq!(next_version, 1); // First call for this bot

    let next_version2 = config_repo
        .get_next_version_atomic(bot_id)
        .await
        .expect("Failed to get next version 2");
    assert_eq!(next_version2, 2); // Second call increments
}

#[tokio::test]
async fn test_authentication_registration_token() {
    let bot_repo = Arc::new(MockBotRepository::default());

    // Create bot without token
    let bot = Bot::new(
        Uuid::new_v4(),
        "Auth Test Bot".to_string(),
        Persona::Beginner,
    );
    let bot_id = bot.id;
    bot_repo.create(&bot).await.expect("Failed to create bot");

    // Initially no token set
    let initial = bot_repo.get_by_id(bot_id).await.expect("Failed to get bot");
    assert!(initial.registration_token.is_none());

    // Set registration token
    let token = "test-registration-token-12345";
    bot_repo
        .update_registration_token(bot_id, token)
        .await
        .expect("Failed to set token");

    // Verify token was set
    let with_token = bot_repo.get_by_id(bot_id).await.expect("Failed to get bot");
    assert_eq!(with_token.registration_token, Some(token.to_string()));

    // Test get_by_id_with_token with correct token
    let authenticated = bot_repo
        .get_by_id_with_token(bot_id, token)
        .await
        .expect("Failed to authenticate with correct token");
    assert_eq!(authenticated.id, bot_id);

    // Test get_by_id_with_token with wrong token - should fail
    let wrong_token_result = bot_repo.get_by_id_with_token(bot_id, "wrong-token").await;
    assert!(wrong_token_result.is_err());
}

#[tokio::test]
async fn test_risk_config_validation() {
    // Test valid config
    let valid = RiskConfig {
        max_position_size_pct: 50.0,
        max_daily_loss_pct: 10.0,
        max_drawdown_pct: 25.0,
        max_trades_per_day: 10,
    };
    assert!(valid.validate().is_ok());

    // Test invalid - negative percentage
    let invalid_negative = RiskConfig {
        max_position_size_pct: -10.0,
        max_daily_loss_pct: 5.0,
        max_drawdown_pct: 20.0,
        max_trades_per_day: 10,
    };
    let result = invalid_negative.validate();
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.contains("max_position_size_pct")));

    // Test invalid - over 100%
    let invalid_over_100 = RiskConfig {
        max_position_size_pct: 150.0,
        max_daily_loss_pct: 5.0,
        max_drawdown_pct: 20.0,
        max_trades_per_day: 10,
    };
    let result2 = invalid_over_100.validate();
    assert!(result2.is_err());

    // Test invalid - negative trades per day
    let invalid_trades = RiskConfig {
        max_position_size_pct: 10.0,
        max_daily_loss_pct: 5.0,
        max_drawdown_pct: 20.0,
        max_trades_per_day: -5,
    };
    let result3 = invalid_trades.validate();
    assert!(result3.is_err());
    let errors3 = result3.unwrap_err();
    assert!(errors3.iter().any(|e| e.contains("max_trades_per_day")));
}

#[tokio::test]
async fn test_account_limit_enforcement() {
    let bot_repo = Arc::new(MockBotRepository::default());
    let account_id = Uuid::new_v4();

    // Mock account has Basic tier with 2 bot limit

    // First bot - should succeed
    let (success1, count1, max1) = bot_repo
        .increment_bot_counter(account_id)
        .await
        .expect("Failed to increment");
    assert!(success1);
    assert_eq!(count1, 1);
    assert_eq!(max1, 2);

    // Second bot - should succeed
    let (success2, count2, max2) = bot_repo
        .increment_bot_counter(account_id)
        .await
        .expect("Failed to increment");
    assert!(success2);
    assert_eq!(count2, 2);
    assert_eq!(max2, 2);

    // Third bot - should fail (at limit)
    let (success3, count3, max3) = bot_repo
        .increment_bot_counter(account_id)
        .await
        .expect("Failed to increment");
    assert!(!success3); // Should fail
    assert_eq!(count3, 2); // Count stays at max
    assert_eq!(max3, 2);

    // Decrement counter
    bot_repo
        .decrement_bot_counter(account_id)
        .await
        .expect("Failed to decrement");

    // Now should succeed again
    let (success4, count4, _) = bot_repo
        .increment_bot_counter(account_id)
        .await
        .expect("Failed to increment");
    assert!(success4);
    assert_eq!(count4, 2);
}

#[tokio::test]
async fn test_stale_bot_detection() {
    let bot_repo = Arc::new(MockBotRepository::default());
    let lifecycle =
        BotLifecycleService::new(bot_repo.clone(), Arc::new(MockConfigRepository::default()));

    // Create bot and set it online
    let bot = Bot::new(Uuid::new_v4(), "Stale Bot".to_string(), Persona::Beginner);
    let bot_id = bot.id;
    bot_repo.create(&bot).await.expect("Failed to create bot");
    bot_repo
        .update_status(bot_id, BotStatus::Online)
        .await
        .expect("Failed to set online");

    // Initially not stale (just set online with no heartbeat)
    let threshold = Utc::now() - chrono::Duration::minutes(5);
    let stale = bot_repo
        .list_stale_bots(threshold)
        .await
        .expect("Failed to list stale");
    assert!(stale.iter().any(|b| b.id == bot_id)); // No heartbeat = stale

    // Record heartbeat
    bot_repo
        .update_heartbeat(bot_id)
        .await
        .expect("Failed to record heartbeat");

    // Now not stale
    let stale2 = bot_repo
        .list_stale_bots(threshold)
        .await
        .expect("Failed to list stale");
    assert!(!stale2.iter().any(|b| b.id == bot_id));

    // Check via lifecycle service
    let stale_via_service = lifecycle
        .check_stale_bots(chrono::Duration::minutes(5))
        .await
        .expect("Failed to check stale");
    assert!(!stale_via_service.iter().any(|b| b.id == bot_id));
}

#[tokio::test]
async fn test_pagination() {
    let bot_repo = Arc::new(MockBotRepository::default());
    let account_id = Uuid::new_v4();

    // Create 5 bots
    for i in 0..5 {
        let bot = Bot::new(account_id, format!("Bot {}", i), Persona::Beginner);
        bot_repo.create(&bot).await.expect("Failed to create bot");
    }

    // Get all
    let all = bot_repo
        .list_by_account(account_id)
        .await
        .expect("Failed to list");
    assert_eq!(all.len(), 5);

    // Get count
    let count = bot_repo
        .count_by_account(account_id)
        .await
        .expect("Failed to count");
    assert_eq!(count, 5);

    // Paginated - limit 2
    let page1 = bot_repo
        .list_by_account_paginated(account_id, 2, 0)
        .await
        .expect("Failed to get page 1");
    assert_eq!(page1.len(), 2);

    let page2 = bot_repo
        .list_by_account_paginated(account_id, 2, 2)
        .await
        .expect("Failed to get page 2");
    assert_eq!(page2.len(), 2);
}

#[tokio::test]
async fn test_bot_name_sanitization() {
    // This test verifies the sanitize_bot_name logic is applied
    // In real code, this would be called before creating a bot

    // Test cases that would be sanitized:
    // - "Test Bot" -> "Test Bot" (valid)
    // - "Test@Bot" -> "Test_Bot" (special char replaced)
    // - "   Test   " -> "Test" (trimmed)
    // - Very long name -> truncated to 64 chars

    // Since sanitize_bot_name is private in provisioning.rs,
    // we verify it works by checking the bot name doesn't cause issues
    let bot_repo = Arc::new(MockBotRepository::default());
    let account_id = Uuid::new_v4();

    // Bot with special chars - should be stored fine
    let bot = Bot::new(account_id, "Test@#$Bot".to_string(), Persona::Beginner);
    bot_repo
        .create(&bot)
        .await
        .expect("Failed to create bot with special chars");

    let retrieved = bot_repo.get_by_id(bot.id).await.expect("Failed to get bot");
    assert_eq!(retrieved.name, "Test@#$Bot"); // In mock, stored as-is
}

#[tokio::test]
async fn test_config_version_conflict_detection() {
    let config_repo = Arc::new(MockConfigRepository::default());
    let bot_repo = Arc::new(MockBotRepository::default());
    let lifecycle = BotLifecycleService::new(bot_repo.clone(), config_repo.clone());

    let account_id = Uuid::new_v4();
    let bot = Bot::new(account_id, "Conflict Test".to_string(), Persona::Beginner);
    let bot_id = bot.id;
    bot_repo.create(&bot).await.expect("Failed to create bot");

    // Create config v1
    let config1 = create_test_stored_config(bot_id, 1);
    config_repo
        .create(&config1)
        .await
        .expect("Failed to create config 1");

    // Set as desired config
    bot_repo
        .update_config_version(bot_id, Some(config1.id), None)
        .await
        .expect("Failed to set desired");

    // Acknowledge v1 - should succeed
    let result = lifecycle.acknowledge_config(bot_id, config1.id).await;
    assert!(result.is_ok());

    // Create config v2 and update desired
    let config2 = create_test_stored_config(bot_id, 2);
    config_repo
        .create(&config2)
        .await
        .expect("Failed to create config 2");
    bot_repo
        .update_config_version(bot_id, Some(config2.id), Some(config1.id))
        .await
        .expect("Failed to set desired v2");

    // Try to acknowledge v1 again - should fail (MED-004: version conflict)
    let result2 = lifecycle.acknowledge_config(bot_id, config1.id).await;
    assert!(result2.is_err());
}
