use axum::{
    extract::{Path, Query, State},
    http::{header::HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use claw_spawn::{
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
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
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

/// CLEAN-004: OpenAPI documentation structure
#[derive(OpenApi)]
#[openapi(
    paths(
        health_check,
        create_account,
        get_account,
        list_bots,
        create_bot,
        get_bot,
        get_bot_config,
        bot_action,
        register_bot,
        get_desired_config,
        acknowledge_config,
        record_heartbeat,
    ),
    components(
        schemas(
            CreateAccountRequest,
            CreateBotRequest,
            BotActionRequest,
            RegisterBotRequest,
            AckConfigRequest,
            BotResponse,
            HealthResponse,
        )
    ),
    tags(
        (name = "Health", description = "Health check endpoints"),
        (name = "Accounts", description = "Account management endpoints"),
        (name = "Bots", description = "Bot management and lifecycle endpoints"),
        (name = "Configuration", description = "Bot configuration endpoints"),
    ),
    info(
        title = "Claw Spawn API",
        version = "0.1.0",
        description = "API for managing trading bot provisioning and lifecycle",
        license(name = "MIT OR Apache-2.0")
    )
)]
struct ApiDoc;

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
        // CLEAN-004: Swagger UI for OpenAPI documentation
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", config.server_host, config.server_port)).await?;
    info!("Server running at http://{}:{}", config.server_host, config.server_port);
    info!("API documentation available at http://{}:{}/docs", config.server_host, config.server_port);
    axum::serve(listener, app).await?;

    Ok(())
}

/// Health check response
#[derive(Serialize, ToSchema)]
struct HealthResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Health check endpoint
/// 
/// Verifies database connectivity and returns service health status.
#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
        (status = 503, description = "Service is unhealthy", body = HealthResponse)
    )
)]
async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    // CLEAN-005: Query DB to verify connectivity
    match sqlx::query("SELECT 1").fetch_one(&state.pool).await {
        Ok(_) => (
            StatusCode::OK, 
            Json(HealthResponse { status: "healthy".to_string(), error: None })
        ),
        Err(e) => {
            error!("Health check failed: DB connectivity issue: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse { 
                    status: "unhealthy".to_string(), 
                    error: Some("Database connectivity failed".to_string()) 
                }),
            )
        }
    }
}

/// Create account request
#[derive(Deserialize, ToSchema)]
struct CreateAccountRequest {
    #[schema(example = "user-123")]
    external_id: String,
    #[schema(example = "pro")]
    tier: String,
}

/// Create a new account
/// 
/// Creates a new account with the specified tier and returns the account ID.
#[utoipa::path(
    post,
    path = "/accounts",
    tag = "Accounts",
    request_body = CreateAccountRequest,
    responses(
        (status = 201, description = "Account created successfully", body = Object),
        (status = 500, description = "Failed to create account", body = Object)
    )
)]
async fn create_account(
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> impl IntoResponse {
    let tier = match req.tier.as_str() {
        "basic" => claw_spawn::domain::SubscriptionTier::Basic,
        "pro" => claw_spawn::domain::SubscriptionTier::Pro,
        _ => claw_spawn::domain::SubscriptionTier::Free,
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

/// Get account by ID
/// 
/// Retrieves account information by ID.
#[utoipa::path(
    get,
    path = "/accounts/{id}",
    tag = "Accounts",
    params(
        ("id" = Uuid, Path, description = "Account ID")
    ),
    responses(
        (status = 501, description = "Not implemented")
    )
)]
async fn get_account(
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Get account not implemented")
}

/// PERF-002: Pagination parameters for list endpoints
/// - limit: Number of items per page (default: 100, max: 1000)
/// - offset: Number of items to skip (default: 0)
#[derive(Deserialize, Debug, IntoParams, ToSchema)]
struct PaginationParams {
    /// Number of items per page (default: 100, max: 1000)
    #[serde(default = "default_limit")]
    #[param(default = 100, maximum = 1000)]
    limit: i64,
    /// Number of items to skip (default: 0)
    #[serde(default)]
    #[param(default = 0)]
    offset: i64,
}

fn default_limit() -> i64 {
    100
}

const MAX_PAGINATION_LIMIT: i64 = 1000;

/// List bots for an account
/// 
/// Returns a paginated list of bots belonging to the specified account.
#[utoipa::path(
    get,
    path = "/accounts/{id}/bots",
    tag = "Bots",
    params(
        ("id" = Uuid, Path, description = "Account ID"),
        PaginationParams
    ),
    responses(
        (status = 200, description = "List of bots", body = [BotResponse]),
        (status = 500, description = "Failed to list bots", body = Object)
    )
)]
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

/// Create bot request
#[derive(Deserialize, ToSchema)]
struct CreateBotRequest {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    account_id: Uuid,
    #[schema(example = "My Trading Bot")]
    name: String,
    #[schema(example = "beginner")]
    persona: String,
    #[schema(example = "majors")]
    asset_focus: String,
    #[schema(example = "trend")]
    algorithm: String,
    #[schema(example = "medium")]
    strictness: String,
    #[schema(example = false)]
    paper_mode: bool,
    #[schema(example = 10.0)]
    max_position_size_pct: f64,
    #[schema(example = 5.0)]
    max_daily_loss_pct: f64,
    #[schema(example = 20.0)]
    max_drawdown_pct: f64,
    #[schema(example = 100)]
    max_trades_per_day: i32,
    #[schema(example = "openai")]
    llm_provider: String,
    #[schema(example = "sk-...")]
    llm_api_key: String,
}

/// Create a new bot
/// 
/// Creates a new trading bot with the specified configuration and provisions a DigitalOcean droplet.
#[utoipa::path(
    post,
    path = "/bots",
    tag = "Bots",
    request_body = CreateBotRequest,
    responses(
        (status = 201, description = "Bot created successfully", body = BotResponse),
        (status = 400, description = "Invalid risk configuration", body = Object),
        (status = 403, description = "Account limit reached", body = Object),
        (status = 429, description = "Rate limited by DigitalOcean", body = Object),
        (status = 500, description = "Failed to create bot", body = Object)
    )
)]
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

/// Get bot by ID
/// 
/// Retrieves detailed information about a specific bot.
#[utoipa::path(
    get,
    path = "/bots/{id}",
    tag = "Bots",
    params(
        ("id" = Uuid, Path, description = "Bot ID")
    ),
    responses(
        (status = 200, description = "Bot found", body = BotResponse),
        (status = 404, description = "Bot not found", body = Object)
    )
)]
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

/// Get bot configuration
/// 
/// Retrieves the current configuration for a bot.
#[utoipa::path(
    get,
    path = "/bots/{id}/config",
    tag = "Configuration",
    params(
        ("id" = Uuid, Path, description = "Bot ID")
    ),
    responses(
        (status = 200, description = "Configuration found", body = Object),
        (status = 404, description = "No config found", body = Object),
        (status = 500, description = "Failed to get config", body = Object)
    )
)]
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

/// Bot action request
#[derive(Deserialize, ToSchema)]
struct BotActionRequest {
    /// Action to perform: pause, resume, redeploy, destroy
    #[schema(example = "pause")]
    action: String,
}

/// Perform bot action
/// 
/// Performs lifecycle actions on a bot: pause, resume, redeploy, or destroy.
#[utoipa::path(
    post,
    path = "/bots/{id}/actions",
    tag = "Bots",
    params(
        ("id" = Uuid, Path, description = "Bot ID")
    ),
    request_body = BotActionRequest,
    responses(
        (status = 200, description = "Action completed successfully", body = Object),
        (status = 400, description = "Invalid action", body = Object),
        (status = 500, description = "Action failed", body = Object)
    )
)]
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
                Json(serde_json::json!({"error": "Action failed" })),
            )
        }
    }
}

/// Get desired config for bot
/// 
/// Retrieves the desired configuration that a bot should apply.
#[utoipa::path(
    get,
    path = "/bot/{id}/config",
    tag = "Configuration",
    params(
        ("id" = Uuid, Path, description = "Bot ID")
    ),
    responses(
        (status = 200, description = "Desired config found", body = Object),
        (status = 404, description = "No desired config", body = Object),
        (status = 500, description = "Failed to get config", body = Object)
    )
)]
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

/// Acknowledge config request
#[derive(Deserialize, ToSchema)]
struct AckConfigRequest {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440001")]
    config_id: Uuid,
}

/// Acknowledge configuration
/// 
/// Bot acknowledges it has applied a configuration version.
#[utoipa::path(
    post,
    path = "/bot/{id}/config_ack",
    tag = "Configuration",
    params(
        ("id" = Uuid, Path, description = "Bot ID")
    ),
    request_body = AckConfigRequest,
    responses(
        (status = 200, description = "Config acknowledged", body = Object),
        (status = 400, description = "Failed to acknowledge config", body = Object)
    )
)]
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

/// Record heartbeat
/// 
/// Records a heartbeat from a bot to indicate it's alive.
#[utoipa::path(
    post,
    path = "/bot/{id}/heartbeat",
    tag = "Bots",
    params(
        ("id" = Uuid, Path, description = "Bot ID")
    ),
    responses(
        (status = 200, description = "Heartbeat recorded", body = Object),
        (status = 500, description = "Failed to record heartbeat", body = Object)
    )
)]
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

/// Register bot request
#[derive(Deserialize, ToSchema)]
struct RegisterBotRequest {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    bot_id: Uuid,
}

/// Register a bot
/// 
/// Bot registration endpoint called by the bot on startup.
/// Requires a valid registration token in the Authorization header.
#[utoipa::path(
    post,
    path = "/bot/register",
    tag = "Bots",
    request_body = RegisterBotRequest,
    responses(
        (status = 200, description = "Bot registered successfully", body = Object),
        (status = 401, description = "Invalid or missing authorization token", body = Object)
    )
)]
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

/// Bot response
#[derive(Serialize, ToSchema)]
struct BotResponse {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    id: Uuid,
    #[schema(example = "550e8400-e29b-41d4-a716-446655440001")]
    account_id: Uuid,
    #[schema(example = "My Trading Bot")]
    name: String,
    #[schema(example = "beginner")]
    persona: String,
    #[schema(example = "online")]
    status: String,
    #[schema(example = 12345678)]
    droplet_id: Option<i64>,
    #[schema(example = "550e8400-e29b-41d4-a716-446655440002")]
    desired_config_version_id: Option<Uuid>,
    #[schema(example = "550e8400-e29b-41d4-a716-446655440002")]
    applied_config_version_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    #[schema(format = "date-time")]
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
