use super::state::AppState;
use crate::application::ProvisioningError;
use crate::domain::{
    Account, AlgorithmMode, AssetFocus, Bot, BotConfig, BotSecrets, Persona, RiskConfig, SignalKnobs,
    StrictnessLevel, TradingConfig,
};
use crate::infrastructure::{AccountRepository, DigitalOceanError};
use axum::{
    extract::{Path, Query, State},
    http::{header, header::HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

pub fn router(state: AppState) -> Router {
    Router::new()
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
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(state)
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|t| !t.is_empty())
}

fn parse_subscription_tier(tier: &str) -> Option<crate::domain::SubscriptionTier> {
    match tier {
        "free" => Some(crate::domain::SubscriptionTier::Free),
        "basic" => Some(crate::domain::SubscriptionTier::Basic),
        "pro" => Some(crate::domain::SubscriptionTier::Pro),
        _ => None,
    }
}

fn parse_persona(persona: &str) -> Option<Persona> {
    match persona {
        "beginner" => Some(Persona::Beginner),
        "tweaker" => Some(Persona::Tweaker),
        "quant_lite" => Some(Persona::QuantLite),
        _ => None,
    }
}

fn parse_asset_focus(asset_focus: &str) -> Option<AssetFocus> {
    match asset_focus {
        "majors" => Some(AssetFocus::Majors),
        "memes" => Some(AssetFocus::Memes),
        _ => None,
    }
}

fn parse_algorithm(algorithm: &str) -> Option<AlgorithmMode> {
    match algorithm {
        "trend" => Some(AlgorithmMode::Trend),
        "mean_reversion" => Some(AlgorithmMode::MeanReversion),
        "breakout" => Some(AlgorithmMode::Breakout),
        _ => None,
    }
}

fn parse_strictness(strictness: &str) -> Option<StrictnessLevel> {
    match strictness {
        "low" => Some(StrictnessLevel::Low),
        "medium" => Some(StrictnessLevel::Medium),
        "high" => Some(StrictnessLevel::High),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extract_bearer_token_happy_path() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer abc123"));
        assert_eq!(extract_bearer_token(&headers), Some("abc123"));
    }

    #[test]
    fn extract_bearer_token_rejects_missing_or_empty() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer_token(&headers), None);

        let mut headers2 = HeaderMap::new();
        headers2.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(extract_bearer_token(&headers2), None);
    }

    #[test]
    fn extract_bearer_token_rejects_wrong_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc123"));
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn parse_invalid_inputs_return_none() {
        assert!(parse_subscription_tier("nope").is_none());
        assert!(parse_persona("nope").is_none());
        assert!(parse_asset_focus("nope").is_none());
        assert!(parse_algorithm("nope").is_none());
        assert!(parse_strictness("nope").is_none());
    }
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
    match sqlx::query("SELECT 1").fetch_one(&state.pool).await {
        Ok(_) => (
            StatusCode::OK,
            Json(HealthResponse {
                status: "healthy".to_string(),
                error: None,
            }),
        ),
        Err(e) => {
            error!(error = %e, "Health check failed: DB connectivity issue");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "unhealthy".to_string(),
                    error: Some("Database connectivity failed".to_string()),
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
#[utoipa::path(
    post,
    path = "/accounts",
    tag = "Accounts",
    request_body = CreateAccountRequest,
    responses(
        (status = 201, description = "Account created successfully", body = Object),
        (status = 400, description = "Invalid subscription tier", body = Object),
        (status = 500, description = "Failed to create account", body = Object)
    )
)]
async fn create_account(State(state): State<AppState>, Json(req): Json<CreateAccountRequest>) -> impl IntoResponse {
    let tier = match parse_subscription_tier(req.tier.as_str()) {
        Some(t) => t,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid subscription tier",
                    "allowed": ["free", "basic", "pro"]
                })),
            );
        }
    };

    let account = Account::new(req.external_id, tier);
    if let Err(e) = state.account_repo.create(&account).await {
        error!(error = %e, "Failed to create account");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to create account"})),
        );
    }

    (StatusCode::CREATED, Json(serde_json::json!({"id": account.id})))
}

#[utoipa::path(
    get,
    path = "/accounts/{id}",
    tag = "Accounts",
    params(("id" = Uuid, Path, description = "Account ID")),
    responses((status = 501, description = "Not implemented"))
)]
async fn get_account(State(_state): State<AppState>, Path(_id): Path<Uuid>) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Get account not implemented")
}

#[derive(Deserialize, Debug, IntoParams, ToSchema)]
struct PaginationParams {
    #[serde(default = "default_limit")]
    #[param(default = 100, maximum = 1000)]
    limit: i64,
    #[serde(default)]
    #[param(default = 0)]
    offset: i64,
}

fn default_limit() -> i64 {
    100
}

const MAX_PAGINATION_LIMIT: i64 = 1000;

#[utoipa::path(
    get,
    path = "/accounts/{id}/bots",
    tag = "Bots",
    params(("id" = Uuid, Path, description = "Account ID"), PaginationParams),
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
    let limit = params.limit.clamp(1, MAX_PAGINATION_LIMIT);
    let offset = params.offset.max(0);

    match state.lifecycle.list_account_bots(account_id, limit, offset).await {
        Ok(bots) => {
            let bot_responses: Vec<BotResponse> = bots.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(serde_json::json!(bot_responses)))
        }
        Err(e) => {
            error!(error = %e, "Failed to list bots");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to list bots"})),
            )
        }
    }
}

#[derive(Deserialize, ToSchema)]
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
async fn create_bot(State(state): State<AppState>, Json(req): Json<CreateBotRequest>) -> impl IntoResponse {
    let persona = match parse_persona(req.persona.as_str()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid persona",
                    "allowed": ["beginner", "tweaker", "quant_lite"]
                })),
            );
        }
    };

    let asset_focus = match parse_asset_focus(req.asset_focus.as_str()) {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid asset_focus",
                    "allowed": ["majors", "memes"]
                })),
            );
        }
    };

    let algorithm = match parse_algorithm(req.algorithm.as_str()) {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid algorithm",
                    "allowed": ["trend", "mean_reversion", "breakout"]
                })),
            );
        }
    };

    let strictness = match parse_strictness(req.strictness.as_str()) {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid strictness",
                    "allowed": ["low", "medium", "high"]
                })),
            );
        }
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
        error!(errors = ?errors, "RiskConfig validation failed");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid risk configuration", "details": errors})),
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
        Ok(bot) => (StatusCode::CREATED, Json(serde_json::json!(BotResponse::from(bot)))),
        Err(ProvisioningError::AccountLimitReached(max)) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": format!("Account limit reached: maximum {} bots allowed", max)
            })),
        ),
        Err(ProvisioningError::DigitalOcean(DigitalOceanError::RateLimited)) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "Rate limited by DigitalOcean, please retry"})),
        ),
        Err(e) => {
            error!(error = %e, "Failed to create bot");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to create bot"})),
            )
        }
    }
}

#[utoipa::path(
    get,
    path = "/bots/{id}",
    tag = "Bots",
    params(("id" = Uuid, Path, description = "Bot ID")),
    responses(
        (status = 200, description = "Bot found", body = BotResponse),
        (status = 404, description = "Bot not found", body = Object)
    )
)]
async fn get_bot(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match state.lifecycle.get_bot(id).await {
        Ok(bot) => (StatusCode::OK, Json(serde_json::json!(BotResponse::from(bot)))),
        Err(_) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Bot not found"}))),
    }
}

#[utoipa::path(
    get,
    path = "/bots/{id}/config",
    tag = "Configuration",
    params(("id" = Uuid, Path, description = "Bot ID")),
    responses(
        (status = 200, description = "Configuration found", body = Object),
        (status = 404, description = "No config found", body = Object),
        (status = 500, description = "Failed to get config", body = Object)
    )
)]
async fn get_bot_config(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match state.lifecycle.get_desired_config(id).await {
        Ok(Some(config)) => (StatusCode::OK, Json(serde_json::json!(config))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No config found"}))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to get config"})),
        ),
    }
}

#[derive(Deserialize, ToSchema)]
struct BotActionRequest {
    action: String,
}

#[utoipa::path(
    post,
    path = "/bots/{id}/actions",
    tag = "Bots",
    params(("id" = Uuid, Path, description = "Bot ID")),
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
            error!(error = %e, "Bot action failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Action failed"})),
            )
        }
    }
}

#[derive(Deserialize, ToSchema)]
struct RegisterBotRequest {
    bot_id: Uuid,
}

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
    let token = match extract_bearer_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Missing or invalid authorization token"})),
            );
        }
    };

    match state.lifecycle.get_bot_with_token(req.bot_id, token).await {
        Ok(bot) => {
            info!(bot_id = %bot.id, "Bot registered successfully");
            (StatusCode::OK, Json(serde_json::json!({"status": "registered"})))
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid bot ID or registration token"})),
        ),
    }
}

#[derive(Deserialize, ToSchema)]
struct AckConfigRequest {
    config_id: Uuid,
}

#[utoipa::path(
    get,
    path = "/bot/{id}/config",
    tag = "Configuration",
    params(("id" = Uuid, Path, description = "Bot ID")),
    responses(
        (status = 200, description = "Desired config found", body = Object),
        (status = 401, description = "Invalid or missing authorization token", body = Object),
        (status = 404, description = "No desired config", body = Object),
        (status = 500, description = "Failed to get config", body = Object)
    )
)]
async fn get_desired_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = match extract_bearer_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Missing or invalid authorization token"})),
            );
        }
    };

    if state.lifecycle.get_bot_with_token(id, token).await.is_err() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid bot ID or registration token"})),
        );
    }

    match state.lifecycle.get_desired_config(id).await {
        Ok(Some(config)) => (StatusCode::OK, Json(serde_json::json!(config))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No desired config"})),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to get config"})),
        ),
    }
}

#[utoipa::path(
    post,
    path = "/bot/{id}/config_ack",
    tag = "Configuration",
    params(("id" = Uuid, Path, description = "Bot ID")),
    request_body = AckConfigRequest,
    responses(
        (status = 200, description = "Config acknowledged", body = Object),
        (status = 401, description = "Invalid or missing authorization token", body = Object),
        (status = 400, description = "Failed to acknowledge config", body = Object)
    )
)]
async fn acknowledge_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<AckConfigRequest>,
) -> impl IntoResponse {
    let token = match extract_bearer_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Missing or invalid authorization token"})),
            );
        }
    };

    if state.lifecycle.get_bot_with_token(id, token).await.is_err() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid bot ID or registration token"})),
        );
    }

    match state.lifecycle.acknowledge_config(id, req.config_id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "acknowledged"})),
        ),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Failed to acknowledge config"})),
        ),
    }
}

#[utoipa::path(
    post,
    path = "/bot/{id}/heartbeat",
    tag = "Bots",
    params(("id" = Uuid, Path, description = "Bot ID")),
    responses(
        (status = 200, description = "Heartbeat recorded", body = Object),
        (status = 401, description = "Invalid or missing authorization token", body = Object),
        (status = 500, description = "Failed to record heartbeat", body = Object)
    )
)]
async fn record_heartbeat(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = match extract_bearer_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Missing or invalid authorization token"})),
            );
        }
    };

    if state.lifecycle.get_bot_with_token(id, token).await.is_err() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid bot ID or registration token"})),
        );
    }

    match state.lifecycle.record_heartbeat(id).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to record heartbeat"})),
        ),
    }
}

#[derive(Serialize, ToSchema)]
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
