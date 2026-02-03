use axum::{
    extract::{Path, Query, State},
    http::{header::HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use cedros_open_spawn::{
    application::{BotLifecycleService, ProvisioningError, ProvisioningService},
    domain::{
        Account, AlgorithmMode, AssetFocus, Bot, BotConfig, BotSecrets, Persona,
        RiskConfig, SignalKnobs, StrictnessLevel, TradingConfig,
    },
    infrastructure::{
        AccountRepository, AppConfig, DigitalOceanClient, DigitalOceanError,
        PostgresAccountRepository, PostgresBotRepository, SecretsEncryption,
    },
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

mod config_repo;
mod droplet_repo;

use config_repo::PostgresConfigRepository;
use droplet_repo::PostgresDropletRepository;

type ProvisioningServiceType = ProvisioningService<
    PostgresAccountRepository,
    PostgresBotRepository,
    PostgresConfigRepository,
    PostgresDropletRepository,
>;

type BotLifecycleServiceType = BotLifecycleService<
    PostgresBotRepository,
    PostgresConfigRepository,
>;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    account_repo: Arc<PostgresAccountRepository>,
    provisioning: Arc<ProvisioningServiceType>,
    lifecycle: Arc<BotLifecycleServiceType>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = AppConfig::from_env()?;
    info!("Starting server on {}:{}", config.server_host, config.server_port);

    let pool = PgPool::connect(&config.database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let encryption = Arc::new(
        SecretsEncryption::new(&config.encryption_key)
            .expect("Failed to initialize encryption"),
    );

    let do_client = Arc::new(
        DigitalOceanClient::new(config.digitalocean_token)
            .expect("Failed to initialize DigitalOcean client"),
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

    let lifecycle = Arc::new(BotLifecycleService::new(
        bot_repo.clone(),
        config_repo.clone(),
    ));

    let state = AppState {
        pool: pool.clone(),
        account_repo,
        provisioning,
        lifecycle,
    };

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/accounts", post(create_account))
        .route("/accounts/:id", get(get_account))
        .route("/accounts/:id/bots", get(list_bots))
        .route("/bots", post(create_bot))
        .route("/bots/:id", get(get_bot))
        .route("/bots/:id/config", get(get_bot_config))
        .route("/bots/:id/actions", post(bot_action))
        .route("/bot/register", post(register_bot))
        .route("/bot/:id/config", get(get_desired_config))
        .route("/bot/:id/config_ack", post(acknowledge_config))
        .route("/bot/:id/heartbeat", post(record_heartbeat))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", config.server_host, config.server_port)).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    // CLEAN-005: Query DB to verify connectivity
    match sqlx::query("SELECT 1").fetch_one(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "healthy"}))),
        Err(e) => {
            error!("Health check failed: DB connectivity issue: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "status": "unhealthy",
                    "error": "Database connectivity failed"
                })),
            )
        }
    }
}

#[derive(Deserialize)]
struct CreateAccountRequest {
    external_id: String,
    tier: String,
}

async fn create_account(
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> impl IntoResponse {
    let tier = match req.tier.as_str() {
        "basic" => cedros_open_spawn::domain::SubscriptionTier::Basic,
        "pro" => cedros_open_spawn::domain::SubscriptionTier::Pro,
        _ => cedros_open_spawn::domain::SubscriptionTier::Free,
    };

    let account = Account::new(req.external_id, tier);
    
    // CRIT-003: Persist account to database before using
    if let Err(e) = state.account_repo.create(&account).await {
        error!("Failed to create account: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to create account" })),
        );
    }
    
    // Account created successfully, return ID
    (StatusCode::CREATED, Json(serde_json::json!({"id": account.id })))
}

async fn get_account(
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Get account not implemented")
}

/// PERF-002: Pagination parameters for list endpoints
/// - limit: Number of items per page (default: 100, max: 1000)
/// - offset: Number of items to skip (default: 0)
#[derive(Deserialize, Debug)]
struct PaginationParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

fn default_limit() -> i64 {
    100
}

const MAX_PAGINATION_LIMIT: i64 = 1000;

async fn list_bots(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    // PERF-002: Clamp limit to max value to prevent abuse
    let limit = params.limit.min(MAX_PAGINATION_LIMIT).max(1);
    let offset = params.offset.max(0);
    
    match state.lifecycle.list_account_bots(account_id, limit, offset).await {
        Ok(bots) => {
            let bot_responses: Vec<BotResponse> = bots.into_iter().map(|b| b.into()).collect();
            (StatusCode::OK, Json(serde_json::json!(bot_responses)))
        }
        Err(e) => {
            error!("Failed to list bots: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to list bots" })),
            )
        }
    }
}

#[derive(Deserialize)]
struct CreateBotRequest {
    account_id: Uuid,
    name: String,
    persona: String,
    asset_focus: String,
    algorithm: String,
    strictness: String,
    paper_mode: bool,
    max_position_size_pct: f64,
    max_daily_loss_pct: f64,
    max_drawdown_pct: f64,
    max_trades_per_day: i32,
    llm_provider: String,
    llm_api_key: String,
}

async fn create_bot(
    State(state): State<AppState>,
    Json(req): Json<CreateBotRequest>,
) -> impl IntoResponse {
    let persona = match req.persona.as_str() {
        "beginner" => Persona::Beginner,
        "tweaker" => Persona::Tweaker,
        "quant_lite" => Persona::QuantLite,
        _ => Persona::Beginner,
    };

    let asset_focus = match req.asset_focus.as_str() {
        "majors" => AssetFocus::Majors,
        "memes" => AssetFocus::Memes,
        _ => AssetFocus::Majors,
    };

    let algorithm = match req.algorithm.as_str() {
        "trend" => AlgorithmMode::Trend,
        "mean_reversion" => AlgorithmMode::MeanReversion,
        "breakout" => AlgorithmMode::Breakout,
        _ => AlgorithmMode::Trend,
    };

    let strictness = match req.strictness.as_str() {
        "low" => StrictnessLevel::Low,
        "medium" => StrictnessLevel::Medium,
        "high" => StrictnessLevel::High,
        _ => StrictnessLevel::Medium,
    };

    let trading_config = TradingConfig {
        asset_focus,
        algorithm,
        strictness,
        paper_mode: req.paper_mode,
        signal_knobs: if matches!(persona, Persona::QuantLite) {
            Some(SignalKnobs {
                volume_confirmation: true,
                volatility_brake: true,
                liquidity_filter: StrictnessLevel::Medium,
                correlation_brake: true,
            })
        } else {
            None
        },
    };

    let risk_config = RiskConfig {
        max_position_size_pct: req.max_position_size_pct,
        max_daily_loss_pct: req.max_daily_loss_pct,
        max_drawdown_pct: req.max_drawdown_pct,
        max_trades_per_day: req.max_trades_per_day,
    };

    if let Err(errors) = risk_config.validate() {
        error!("RiskConfig validation failed: {:?}", errors);
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Invalid risk configuration",
                "details": errors
            })),
        );
    }

    let config = BotConfig {
        id: Uuid::new_v4(),
        bot_id: Uuid::new_v4(),
        version: 1,
        trading_config,
        risk_config,
        secrets: BotSecrets {
            llm_provider: req.llm_provider,
            llm_api_key: req.llm_api_key,
        },
        created_at: chrono::Utc::now(),
    };

    match state
        .provisioning
        .create_bot(req.account_id, req.name, persona, config)
        .await
    {
        Ok(bot) => {
            let response: BotResponse = bot.into();
            (StatusCode::CREATED, Json(serde_json::json!(response)))
        }
        Err(ProvisioningError::AccountLimitReached(max)) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": format!("Account limit reached: maximum {} bots allowed", max)
            })),
        ),
        Err(ProvisioningError::DigitalOcean(DigitalOceanError::RateLimited)) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "Rate limited by DigitalOcean, please retry"
            })),
        ),
        Err(e) => {
            error!("Failed to create bot: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to create bot" })),
            )
        }
    }
}

async fn get_bot(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.lifecycle.get_bot(id).await {
        Ok(bot) => {
            let response: BotResponse = bot.into();
            (StatusCode::OK, Json(serde_json::json!(response)))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bot not found" })),
        ),
    }
}

async fn get_bot_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.lifecycle.get_desired_config(id).await {
        Ok(Some(config)) => (StatusCode::OK, Json(serde_json::json!(config))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No config found" })),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to get config" })),
        ),
    }
}

#[derive(Deserialize)]
struct BotActionRequest {
    action: String,
}

async fn bot_action(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<BotActionRequest>,
) -> impl IntoResponse {
    let result = match req.action.as_str() {
        "pause" => state.provisioning.pause_bot(id).await,
        "resume" => state.provisioning.resume_bot(id).await,
        "redeploy" => state.provisioning.redeploy_bot(id).await,
        "destroy" => state.provisioning.destroy_bot(id).await,
        _ => Err(ProvisioningError::InvalidConfig("Unknown action".to_string())),
    };

    match result {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))),
        Err(e) => {
            error!("Bot action failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Action failed"})),
            )
        }
    }
}

async fn get_desired_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.lifecycle.get_desired_config(id).await {
        Ok(Some(config)) => (StatusCode::OK, Json(serde_json::json!(config))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No desired config" })),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to get config" })),
        ),
    }
}

#[derive(Deserialize)]
struct AckConfigRequest {
    config_id: Uuid,
}

async fn acknowledge_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<AckConfigRequest>,
) -> impl IntoResponse {
    match state.lifecycle.acknowledge_config(id, req.config_id).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "acknowledged"}))),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Failed to acknowledge config" })),
        ),
    }
}

async fn record_heartbeat(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.lifecycle.record_heartbeat(id).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to record heartbeat" })),
        ),
    }
}

#[derive(Deserialize)]
struct RegisterBotRequest {
    bot_id: Uuid,
}

async fn register_bot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterBotRequest>,
) -> impl IntoResponse {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Missing or invalid authorization token" })),
            );
        }
    };

    if token.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid authorization token" })),
        );
    }

    // CRIT-001: Validate registration token against stored token
    match state.lifecycle.get_bot_with_token(req.bot_id, token).await {
        Ok(bot) => {
            info!("Bot {} registered successfully with valid token", bot.id);
            (StatusCode::OK, Json(serde_json::json!({"status": "registered"})))
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid bot ID or registration token" })),
        ),
    }
}

#[derive(Serialize)]
struct BotResponse {
    id: Uuid,
    account_id: Uuid,
    name: String,
    persona: String,
    status: String,
    droplet_id: Option<i64>,
    desired_config_version_id: Option<Uuid>,
    applied_config_version_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    last_heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<Bot> for BotResponse {
    fn from(bot: Bot) -> Self {
        Self {
            id: bot.id,
            account_id: bot.account_id,
            name: bot.name,
            persona: bot.persona.to_string(),
            status: bot.status.to_string(),
            droplet_id: bot.droplet_id,
            desired_config_version_id: bot.desired_config_version_id,
            applied_config_version_id: bot.applied_config_version_id,
            created_at: bot.created_at,
            updated_at: bot.updated_at,
            last_heartbeat_at: bot.last_heartbeat_at,
        }
    }
}
