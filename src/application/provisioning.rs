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
const RETRY_ATTEMPTS: usize = 3;
const RETRY_DELAYS_MS: [u64; RETRY_ATTEMPTS - 1] = [100, 200];

/// REL-001: Retry an async operation with exponential backoff
/// Logs each retry attempt with structured context
async fn retry_with_backoff<F, Fut, T, E>(operation_name: &str, bot_id: Uuid, f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    // Retry with delays between attempts; final attempt has no delay.
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
                    "Operation failed, will retry after {}ms",
                    delay_ms
                );
                sleep(Duration::from_millis(*delay_ms)).await;
            }
        }
    }

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

fn should_rollback_create_failure(err: &ProvisioningError) -> bool {
    !matches!(
        err,
        ProvisioningError::DigitalOcean(DigitalOceanError::RateLimited)
    )
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
    if trimmed.chars().count() > MAX_BOT_NAME_LENGTH {
        trimmed.chars().take(MAX_BOT_NAME_LENGTH).collect()
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

    // janebot-cli customization
    customizer_repo_url: String,
    customizer_ref: String,
    customizer_workspace_dir: String,
    customizer_agent_name: String,
    customizer_owner_name: String,
    customizer_skip_qmd: bool,
    customizer_skip_cron: bool,
    customizer_skip_git: bool,
    customizer_skip_heartbeat: bool,

    // Droplet toolchain/bootstrap customization
    toolchain_node_major: u8,
    toolchain_install_pnpm: bool,
    toolchain_pnpm_version: String,
    toolchain_install_rust: bool,
    toolchain_rust_toolchain: String,
    toolchain_extra_apt_packages: String,
    toolchain_global_npm_packages: String,
    toolchain_cargo_crates: String,
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct NoopAccountRepo;
    #[async_trait]
    impl AccountRepository for NoopAccountRepo {
        async fn create(&self, _account: &crate::domain::Account) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_by_id(&self, _id: Uuid) -> Result<crate::domain::Account, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_by_external_id(
            &self,
            _external_id: &str,
        ) -> Result<crate::domain::Account, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_subscription(
            &self,
            _id: Uuid,
            _tier: crate::domain::SubscriptionTier,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
    }

    #[derive(Default)]
    struct NoopBotRepo;
    #[async_trait]
    impl BotRepository for NoopBotRepo {
        async fn create(&self, _bot: &Bot) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_by_id(&self, _id: Uuid) -> Result<Bot, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_by_id_with_token(
            &self,
            _id: Uuid,
            _token: &str,
        ) -> Result<Bot, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn list_by_account(&self, _account_id: Uuid) -> Result<Vec<Bot>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn list_by_account_paginated(
            &self,
            _account_id: Uuid,
            _limit: i64,
            _offset: i64,
        ) -> Result<Vec<Bot>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn count_by_account(&self, _account_id: Uuid) -> Result<i64, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: BotStatus,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_droplet(
            &self,
            _bot_id: Uuid,
            _droplet_id: Option<i64>,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_config_version(
            &self,
            _bot_id: Uuid,
            _desired: Option<Uuid>,
            _applied: Option<Uuid>,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_heartbeat(&self, _bot_id: Uuid) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_registration_token(
            &self,
            _bot_id: Uuid,
            _token: &str,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn hard_delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn increment_bot_counter(
            &self,
            _account_id: Uuid,
        ) -> Result<(bool, i32, i32), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn decrement_bot_counter(&self, _account_id: Uuid) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn list_stale_bots(
            &self,
            _threshold: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<Bot>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
    }

    #[derive(Default)]
    struct NoopConfigRepo;
    #[async_trait]
    impl ConfigRepository for NoopConfigRepo {
        async fn create(&self, _config: &StoredBotConfig) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_by_id(&self, _id: Uuid) -> Result<StoredBotConfig, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_latest_for_bot(
            &self,
            _bot_id: Uuid,
        ) -> Result<Option<StoredBotConfig>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn list_by_bot(
            &self,
            _bot_id: Uuid,
        ) -> Result<Vec<StoredBotConfig>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_next_version_atomic(&self, _bot_id: Uuid) -> Result<i32, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
    }

    #[derive(Default)]
    struct NoopDropletRepo;
    #[async_trait]
    impl DropletRepository for NoopDropletRepo {
        async fn create(&self, _droplet: &crate::domain::Droplet) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_by_id(&self, _id: i64) -> Result<crate::domain::Droplet, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_bot_assignment(
            &self,
            _droplet_id: i64,
            _bot_id: Option<Uuid>,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_status(
            &self,
            _droplet_id: i64,
            _status: &str,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_ip(
            &self,
            _droplet_id: i64,
            _ip: Option<String>,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn mark_destroyed(&self, _droplet_id: i64) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
    }

    #[derive(Default)]
    struct HappyAccountRepo;
    #[async_trait]
    impl AccountRepository for HappyAccountRepo {
        async fn create(&self, _account: &crate::domain::Account) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn get_by_id(&self, id: Uuid) -> Result<crate::domain::Account, RepositoryError> {
            let now = Utc::now();
            Ok(crate::domain::Account {
                id,
                external_id: "test-account".to_string(),
                subscription_tier: crate::domain::SubscriptionTier::Basic,
                max_bots: 2,
                created_at: now,
                updated_at: now,
            })
        }
        async fn get_by_external_id(
            &self,
            _external_id: &str,
        ) -> Result<crate::domain::Account, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_subscription(
            &self,
            _id: Uuid,
            _tier: crate::domain::SubscriptionTier,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
    }

    #[derive(Default)]
    struct RollbackTrackingBotRepo {
        created: Mutex<HashSet<Uuid>>,
        hard_deleted: AtomicUsize,
        decremented: AtomicUsize,
    }
    #[async_trait]
    impl BotRepository for RollbackTrackingBotRepo {
        async fn create(&self, bot: &Bot) -> Result<(), RepositoryError> {
            self.created.lock().expect("lock").insert(bot.id);
            Ok(())
        }
        async fn get_by_id(&self, _id: Uuid) -> Result<Bot, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_by_id_with_token(
            &self,
            _id: Uuid,
            _token: &str,
        ) -> Result<Bot, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn list_by_account(&self, _account_id: Uuid) -> Result<Vec<Bot>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn list_by_account_paginated(
            &self,
            _account_id: Uuid,
            _limit: i64,
            _offset: i64,
        ) -> Result<Vec<Bot>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn count_by_account(&self, _account_id: Uuid) -> Result<i64, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: BotStatus,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_droplet(
            &self,
            _bot_id: Uuid,
            _droplet_id: Option<i64>,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_config_version(
            &self,
            _bot_id: Uuid,
            _desired: Option<Uuid>,
            _applied: Option<Uuid>,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_heartbeat(&self, _bot_id: Uuid) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn update_registration_token(
            &self,
            _bot_id: Uuid,
            _token: &str,
        ) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn hard_delete(&self, id: Uuid) -> Result<(), RepositoryError> {
            let _ = self.created.lock().expect("lock").remove(&id);
            self.hard_deleted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn increment_bot_counter(
            &self,
            _account_id: Uuid,
        ) -> Result<(bool, i32, i32), RepositoryError> {
            Ok((true, 1, 2))
        }
        async fn decrement_bot_counter(&self, _account_id: Uuid) -> Result<(), RepositoryError> {
            self.decremented.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn list_stale_bots(
            &self,
            _threshold: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<Bot>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
    }

    #[derive(Default)]
    struct FailingConfigCreateRepo;
    #[async_trait]
    impl ConfigRepository for FailingConfigCreateRepo {
        async fn create(&self, _config: &StoredBotConfig) -> Result<(), RepositoryError> {
            Err(RepositoryError::InvalidData(
                "forced config create failure".to_string(),
            ))
        }
        async fn get_by_id(&self, _id: Uuid) -> Result<StoredBotConfig, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_latest_for_bot(
            &self,
            _bot_id: Uuid,
        ) -> Result<Option<StoredBotConfig>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn list_by_bot(
            &self,
            _bot_id: Uuid,
        ) -> Result<Vec<StoredBotConfig>, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
        async fn get_next_version_atomic(&self, _bot_id: Uuid) -> Result<i32, RepositoryError> {
            Err(RepositoryError::InvalidData("noop".to_string()))
        }
    }

    #[test]
    fn f001_user_data_does_not_enable_xtrace() {
        let encryption = Arc::new(
            SecretsEncryption::new("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=")
                .expect("valid test key"),
        );
        let do_client = Arc::new(DigitalOceanClient::new("test-token".to_string()).unwrap());

        let svc: ProvisioningService<
            NoopAccountRepo,
            NoopBotRepo,
            NoopConfigRepo,
            NoopDropletRepo,
        > = ProvisioningService::new(
            do_client,
            Arc::new(NoopAccountRepo),
            Arc::new(NoopBotRepo),
            Arc::new(NoopConfigRepo),
            Arc::new(NoopDropletRepo),
            encryption,
            "ubuntu-22-04-x64".to_string(),
            "https://example.invalid".to_string(),
            "https://github.com/janebot2026/janebot-cli.git".to_string(),
            "4b170b4aa31f79bda84f7383b3992ca8681d06d3".to_string(),
            "/opt/openclaw/workspace".to_string(),
            "Jane".to_string(),
            "Cedros".to_string(),
            true,
            true,
            true,
            true,
            20,
            true,
            "".to_string(),
            true,
            "stable".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
        );

        let bot_id = Uuid::new_v4();
        let user_data = svc.test_only_generate_user_data("reg-token", bot_id);
        assert!(!user_data.lines().any(|l| l.trim() == "set -x"));

        let embedded = include_str!("../../scripts/openclaw-bootstrap.sh");
        assert!(!embedded.lines().any(|l| l.trim() == "set -x"));
    }

    #[test]
    fn f002_user_data_exports_customizer_and_toolchain_values() {
        let encryption = Arc::new(
            SecretsEncryption::new("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=")
                .expect("valid test key"),
        );
        let do_client = Arc::new(DigitalOceanClient::new("test-token".to_string()).unwrap());

        let svc: ProvisioningService<
            NoopAccountRepo,
            NoopBotRepo,
            NoopConfigRepo,
            NoopDropletRepo,
        > = ProvisioningService::new(
            do_client,
            Arc::new(NoopAccountRepo),
            Arc::new(NoopBotRepo),
            Arc::new(NoopConfigRepo),
            Arc::new(NoopDropletRepo),
            encryption,
            "ubuntu-22-04-x64".to_string(),
            "https://control.example".to_string(),
            "https://example.com/customizer.git".to_string(),
            "custom-ref".to_string(),
            "/tmp/workspace".to_string(),
            "AgentX".to_string(),
            "OwnerY".to_string(),
            false,
            true,
            false,
            true,
            20,
            true,
            "9.12.0".to_string(),
            true,
            "stable".to_string(),
            "ripgrep fd-find".to_string(),
            "@openclaw/special-cli".to_string(),
            "cargo-binstall".to_string(),
        );

        let bot_id = Uuid::new_v4();
        let user_data = svc.test_only_generate_user_data("reg-token", bot_id);
        assert!(
            user_data.contains("export CUSTOMIZER_REPO_URL=\"https://example.com/customizer.git\"")
        );
        assert!(user_data.contains("export CUSTOMIZER_REF=\"custom-ref\""));
        assert!(user_data.contains("export TOOLCHAIN_NODE_MAJOR=\"20\""));
        assert!(user_data.contains("export TOOLCHAIN_INSTALL_PNPM=\"true\""));
        assert!(user_data.contains("export TOOLCHAIN_PNPM_VERSION=\"9.12.0\""));
        assert!(user_data.contains("export TOOLCHAIN_INSTALL_RUST=\"true\""));
        assert!(user_data.contains("export TOOLCHAIN_RUST_TOOLCHAIN=\"stable\""));
        assert!(user_data.contains("export TOOLCHAIN_EXTRA_APT_PACKAGES=\"ripgrep fd-find\""));
        assert!(
            user_data.contains("export TOOLCHAIN_GLOBAL_NPM_PACKAGES=\"@openclaw/special-cli\"")
        );
        assert!(user_data.contains("export TOOLCHAIN_CARGO_CRATES=\"cargo-binstall\""));
        assert!(user_data.contains("# Start of embedded bootstrap script"));
        assert!(user_data.contains("# OpenClaw Bot Bootstrap Script"));

        let embedded = include_str!("../../scripts/openclaw-bootstrap.sh");
        assert!(embedded.contains("<< EOFSERVICE"));
        assert!(!embedded.contains("<< 'EOFSERVICE'"));
        assert!(embedded.contains("HB_RESULT=$(send_heartbeat || echo \"000\")"));
    }

    struct TestErr;
    impl std::fmt::Display for TestErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "test error")
        }
    }

    #[tokio::test]
    async fn f004_retry_with_backoff_uses_exact_attempt_count() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();

        let res: Result<(), TestErr> = retry_with_backoff("test_op", Uuid::nil(), move || {
            let calls3 = calls2.clone();
            async move {
                calls3.fetch_add(1, Ordering::SeqCst);
                Err(TestErr)
            }
        })
        .await;

        assert!(res.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), RETRY_ATTEMPTS);
    }

    #[tokio::test]
    async fn f005_create_bot_rolls_back_partial_state_when_config_create_fails() {
        let encryption = Arc::new(
            SecretsEncryption::new("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=")
                .expect("valid test key"),
        );
        let do_client = Arc::new(DigitalOceanClient::new("test-token".to_string()).unwrap());

        let bot_repo = Arc::new(RollbackTrackingBotRepo::default());

        let svc: ProvisioningService<
            HappyAccountRepo,
            RollbackTrackingBotRepo,
            FailingConfigCreateRepo,
            NoopDropletRepo,
        > = ProvisioningService::new(
            do_client,
            Arc::new(HappyAccountRepo),
            bot_repo.clone(),
            Arc::new(FailingConfigCreateRepo),
            Arc::new(NoopDropletRepo),
            encryption,
            "ubuntu-22-04-x64".to_string(),
            "https://example.invalid".to_string(),
            "https://github.com/janebot2026/janebot-cli.git".to_string(),
            "4b170b4aa31f79bda84f7383b3992ca8681d06d3".to_string(),
            "/opt/openclaw/workspace".to_string(),
            "Jane".to_string(),
            "Cedros".to_string(),
            true,
            true,
            true,
            true,
            20,
            true,
            "".to_string(),
            true,
            "stable".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
        );

        let account_id = Uuid::new_v4();
        let res = svc
            .create_bot(
                account_id,
                "rollback-target".to_string(),
                Persona::Beginner,
                BotConfig {
                    id: Uuid::new_v4(),
                    bot_id: Uuid::new_v4(),
                    version: 1,
                    trading_config: crate::domain::TradingConfig {
                        asset_focus: crate::domain::AssetFocus::Majors,
                        algorithm: crate::domain::AlgorithmMode::Trend,
                        strictness: crate::domain::StrictnessLevel::Medium,
                        paper_mode: true,
                        signal_knobs: None,
                    },
                    risk_config: crate::domain::RiskConfig {
                        max_position_size_pct: 10.0,
                        max_daily_loss_pct: 5.0,
                        max_drawdown_pct: 10.0,
                        max_trades_per_day: 10,
                    },
                    secrets: crate::domain::BotSecrets {
                        llm_provider: "test".to_string(),
                        llm_api_key: "test-key".to_string(),
                    },
                    created_at: Utc::now(),
                },
            )
            .await;

        assert!(res.is_err());
        assert_eq!(bot_repo.hard_deleted.load(Ordering::SeqCst), 1);
        assert_eq!(bot_repo.decremented.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn f006_sanitize_bot_name_truncates_multibyte_input_safely() {
        let name = "é".repeat(MAX_BOT_NAME_LENGTH + 12);
        let sanitized = sanitize_bot_name(&name);
        assert_eq!(sanitized.chars().count(), MAX_BOT_NAME_LENGTH);
    }

    #[test]
    fn f002_should_rollback_create_failure_only_for_fatal_errors() {
        assert!(should_rollback_create_failure(&ProvisioningError::Repository(
            RepositoryError::InvalidData("db".to_string())
        )));
        assert!(!should_rollback_create_failure(
            &ProvisioningError::DigitalOcean(DigitalOceanError::RateLimited)
        ));
    }
}

impl<A, B, C, D> ProvisioningService<A, B, C, D>
where
    A: AccountRepository,
    B: BotRepository,
    C: ConfigRepository,
    D: DropletRepository,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        do_client: Arc<DigitalOceanClient>,
        account_repo: Arc<A>,
        bot_repo: Arc<B>,
        config_repo: Arc<C>,
        droplet_repo: Arc<D>,
        encryption: Arc<SecretsEncryption>,
        openclaw_image: String,
        control_plane_url: String,

        customizer_repo_url: String,
        customizer_ref: String,
        customizer_workspace_dir: String,
        customizer_agent_name: String,
        customizer_owner_name: String,
        customizer_skip_qmd: bool,
        customizer_skip_cron: bool,
        customizer_skip_git: bool,
        customizer_skip_heartbeat: bool,
        toolchain_node_major: u8,
        toolchain_install_pnpm: bool,
        toolchain_pnpm_version: String,
        toolchain_install_rust: bool,
        toolchain_rust_toolchain: String,
        toolchain_extra_apt_packages: String,
        toolchain_global_npm_packages: String,
        toolchain_cargo_crates: String,
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

            customizer_repo_url,
            customizer_ref,
            customizer_workspace_dir,
            customizer_agent_name,
            customizer_owner_name,
            customizer_skip_qmd,
            customizer_skip_cron,
            customizer_skip_git,
            customizer_skip_heartbeat,
            toolchain_node_major,
            toolchain_install_pnpm,
            toolchain_pnpm_version,
            toolchain_install_rust,
            toolchain_rust_toolchain,
            toolchain_extra_apt_packages,
            toolchain_global_npm_packages,
            toolchain_cargo_crates,
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
        let (success, _current_count, max_count) =
            self.bot_repo.increment_bot_counter(account_id).await?;

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

        if let Err(ref err) = result {
            if !should_rollback_create_failure(err) {
                return result.map(|_| bot);
            }

            if let Err(e) = self.bot_repo.hard_delete(bot.id).await {
                if !matches!(e, RepositoryError::NotFound(_)) {
                    error!(
                        account_id = %account_id,
                        bot_id = %bot.id,
                        error = %e,
                        "Failed to rollback bot row after failed creation"
                    );
                }
            }

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

    async fn spawn_bot(
        &self,
        bot: &mut Bot,
        config: &StoredBotConfig,
    ) -> Result<(), ProvisioningError> {
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
        self.bot_repo
            .update_registration_token(bot.id, &registration_token)
            .await?;
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
                self.bot_repo
                    .update_status(bot.id, BotStatus::Pending)
                    .await?;
                bot.status = BotStatus::Pending;
                return Err(DigitalOceanError::RateLimited.into());
            }
            Err(e) => {
                error!(
                    bot_id = %bot.id,
                    error = %e,
                    "Failed to create droplet for bot"
                );
                self.bot_repo
                    .update_status(bot.id, BotStatus::Error)
                    .await?;
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
            self.bot_repo
                .update_droplet(bot.id, Some(droplet.id))
                .await?;
            Ok(())
        }
        .await;

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

    fn generate_user_data(
        &self,
        registration_token: &str,
        bot_id: Uuid,
        _config: &StoredBotConfig,
    ) -> String {
        // Read the bootstrap script and prepend environment variables
        let bootstrap_script = include_str!("../../scripts/openclaw-bootstrap.sh");

        // CRIT-006: Use configured control plane URL instead of hardcoded value
        format!(
            r##"#!/bin/bash
# OpenClaw Bot Bootstrap for Bot {}
set -e

# NOTE: Do not enable `set -x` (xtrace). This user-data includes secrets
# (registration token) and xtrace would leak them into cloud-init logs.

export REGISTRATION_TOKEN="{}"
export BOT_ID="{}"
export CONTROL_PLANE_URL="{}"

# Workspace/customization (janebot-cli)
export CUSTOMIZER_REPO_URL="{}"
export CUSTOMIZER_REF="{}"
export CUSTOMIZER_WORKSPACE_DIR="{}"
export CUSTOMIZER_AGENT_NAME="{}"
export CUSTOMIZER_OWNER_NAME="{}"
export CUSTOMIZER_SKIP_QMD="{}"
export CUSTOMIZER_SKIP_CRON="{}"
export CUSTOMIZER_SKIP_GIT="{}"
export CUSTOMIZER_SKIP_HEARTBEAT="{}"

# Toolchain/bootstrap customization
export TOOLCHAIN_NODE_MAJOR="{}"
export TOOLCHAIN_INSTALL_PNPM="{}"
export TOOLCHAIN_PNPM_VERSION="{}"
export TOOLCHAIN_INSTALL_RUST="{}"
export TOOLCHAIN_RUST_TOOLCHAIN="{}"
export TOOLCHAIN_EXTRA_APT_PACKAGES="{}"
export TOOLCHAIN_GLOBAL_NPM_PACKAGES="{}"
export TOOLCHAIN_CARGO_CRATES="{}"

# Start of embedded bootstrap script
{}
"##,
            bot_id,
            registration_token,
            bot_id,
            self.control_plane_url,
            self.customizer_repo_url,
            self.customizer_ref,
            self.customizer_workspace_dir,
            self.customizer_agent_name,
            self.customizer_owner_name,
            self.customizer_skip_qmd,
            self.customizer_skip_cron,
            self.customizer_skip_git,
            self.customizer_skip_heartbeat,
            self.toolchain_node_major,
            self.toolchain_install_pnpm,
            self.toolchain_pnpm_version,
            self.toolchain_install_rust,
            self.toolchain_rust_toolchain,
            self.toolchain_extra_apt_packages,
            self.toolchain_global_npm_packages,
            self.toolchain_cargo_crates,
            bootstrap_script
        )
    }

    #[cfg(test)]
    fn test_only_generate_user_data(&self, registration_token: &str, bot_id: Uuid) -> String {
        // Helper to keep tests focused without additional config setup.
        self.generate_user_data(
            registration_token,
            bot_id,
            &StoredBotConfig {
                id: Uuid::new_v4(),
                bot_id,
                version: 1,
                trading_config: crate::domain::TradingConfig {
                    asset_focus: crate::domain::AssetFocus::Majors,
                    algorithm: crate::domain::AlgorithmMode::Trend,
                    strictness: crate::domain::StrictnessLevel::Medium,
                    paper_mode: true,
                    signal_knobs: None,
                },
                risk_config: crate::domain::RiskConfig {
                    max_position_size_pct: 10.0,
                    max_daily_loss_pct: 5.0,
                    max_drawdown_pct: 10.0,
                    max_trades_per_day: 10,
                },
                secrets: crate::domain::EncryptedBotSecrets {
                    llm_provider: "test".to_string(),
                    llm_api_key_encrypted: vec![1, 2, 3],
                },
                created_at: chrono::Utc::now(),
            },
        )
    }

    fn generate_registration_token(&self, _bot_id: Uuid) -> String {
        let mut token = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token);
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, token)
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
                    if let Err(e) = retry_with_backoff("mark_destroyed", bot_id, || {
                        self.droplet_repo.mark_destroyed(droplet_id)
                    })
                    .await
                    {
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
                    if let Err(e) = retry_with_backoff("mark_destroyed", bot_id, || {
                        self.droplet_repo.mark_destroyed(droplet_id)
                    })
                    .await
                    {
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
        if let Err(e) = retry_with_backoff("update_droplet", bot_id, || {
            self.bot_repo.update_droplet(bot_id, None)
        })
        .await
        {
            error!(
                bot_id = %bot_id,
                error = %e,
                "Failed to update bot droplet reference after retries"
            );
            return Err(e.into());
        }

        if let Err(e) =
            retry_with_backoff("delete_bot", bot_id, || self.bot_repo.delete(bot_id)).await
        {
            error!(
                bot_id = %bot_id,
                error = %e,
                "Failed to delete bot after retries"
            );
            return Err(e.into());
        }

        // CRIT-002: Decrement bot counter when bot is destroyed
        // REL-001: Retry counter decrement
        if let Err(e) = retry_with_backoff("decrement_bot_counter", bot_id, || {
            self.bot_repo.decrement_bot_counter(bot.account_id)
        })
        .await
        {
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

        self.bot_repo
            .update_status(bot_id, BotStatus::Paused)
            .await?;
        Ok(())
    }

    pub async fn resume_bot(&self, bot_id: Uuid) -> Result<(), ProvisioningError> {
        let bot = self.bot_repo.get_by_id(bot_id).await?;

        if bot.status != BotStatus::Paused {
            return Err(ProvisioningError::InvalidConfig(format!(
                "Bot {} is not in paused state (current: {:?})",
                bot_id, bot.status
            )));
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
                            info!(
                                "Droplet {} for bot {} is already active",
                                droplet_id, bot_id
                            );
                        }
                        crate::domain::DropletStatus::New => {
                            // Droplet is still being created, not ready
                            return Err(ProvisioningError::InvalidConfig(format!(
                                "Droplet {} is still being created, cannot resume yet",
                                droplet_id
                            )));
                        }
                        _ => {
                            return Err(ProvisioningError::InvalidConfig(format!(
                                "Droplet {} is in state {:?}, cannot resume",
                                droplet_id, droplet.status
                            )));
                        }
                    }
                }
                Err(DigitalOceanError::NotFound(_)) => {
                    return Err(ProvisioningError::InvalidConfig(format!(
                        "Droplet {} for bot {} no longer exists in DigitalOcean",
                        droplet_id, bot_id
                    )));
                }
                Err(e) => return Err(e.into()),
            }
        } else {
            return Err(ProvisioningError::InvalidConfig(format!(
                "Bot {} has no associated droplet",
                bot_id
            )));
        }

        self.bot_repo
            .update_status(bot_id, BotStatus::Online)
            .await?;
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
        let config = self
            .config_repo
            .get_latest_for_bot(bot_id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::InvalidConfig("No config found for redeployment".to_string())
            })?;

        bot.droplet_id = None;
        self.spawn_bot(&mut bot, &config).await?;

        info!("Successfully redeployed bot {}", bot_id);
        Ok(())
    }

}
