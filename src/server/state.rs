use crate::application::{BotLifecycleService, ProvisioningService};
use crate::infrastructure::{
    AppConfig, DigitalOceanClient, PostgresAccountRepository, PostgresBotRepository,
    PostgresConfigRepository, PostgresDropletRepository, SecretsEncryption,
};
use anyhow::Context;
use sqlx::PgPool;
use std::sync::Arc;

pub type ProvisioningServiceType = ProvisioningService<
    PostgresAccountRepository,
    PostgresBotRepository,
    PostgresConfigRepository,
    PostgresDropletRepository,
>;

pub type BotLifecycleServiceType = BotLifecycleService<PostgresBotRepository, PostgresConfigRepository>;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub account_repo: Arc<PostgresAccountRepository>,
    pub provisioning: Arc<ProvisioningServiceType>,
    pub lifecycle: Arc<BotLifecycleServiceType>,
}

/// Build full state from config + an existing pool.
///
/// Intended for embedding into a larger service that already manages a `PgPool`.
pub async fn build_state_with_pool(
    config: AppConfig,
    pool: PgPool,
    run_migrations: bool,
) -> anyhow::Result<AppState> {
    if run_migrations {
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("run migrations")?;
    }

    let encryption = Arc::new(
        SecretsEncryption::new(&config.encryption_key).context("init encryption")?,
    );

    let do_client = Arc::new(
        DigitalOceanClient::new(config.digitalocean_token).context("init DigitalOcean client")?,
    );

    let account_repo = Arc::new(PostgresAccountRepository::new(pool.clone()));
    let bot_repo = Arc::new(PostgresBotRepository::new(pool.clone()));
    let config_repo = Arc::new(PostgresConfigRepository::new(pool.clone()));
    let droplet_repo = Arc::new(PostgresDropletRepository::new(pool.clone()));

    let provisioning = Arc::new(ProvisioningService::new(
        do_client,
        account_repo.clone(),
        bot_repo.clone(),
        config_repo.clone(),
        droplet_repo.clone(),
        encryption,
        config.openclaw_image,
        config.control_plane_url,
    ));

    let lifecycle = Arc::new(BotLifecycleService::new(bot_repo.clone(), config_repo.clone()));

    Ok(AppState {
        pool,
        account_repo,
        provisioning,
        lifecycle,
    })
}

/// Build state for the standalone server.
///
/// Creates the `PgPool`, runs migrations, and wires repositories/services.
pub async fn build_state_from_env(config: AppConfig) -> anyhow::Result<AppState> {
    let pool = PgPool::connect(&config.database_url)
        .await
        .context("connect database")?;
    build_state_with_pool(config, pool, true).await
}
