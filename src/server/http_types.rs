use crate::domain::Bot;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub(super) struct HealthResponse {
    pub(super) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub(super) struct CreateAccountRequest {
    #[schema(example = "user-123")]
    pub(super) external_id: String,
    #[schema(example = "pro")]
    pub(super) tier: String,
}

#[derive(Deserialize, Debug, IntoParams, ToSchema)]
pub(super) struct PaginationParams {
    #[serde(default = "default_limit")]
    #[param(default = 100, maximum = 1000)]
    pub(super) limit: i64,
    #[serde(default)]
    #[param(default = 0)]
    pub(super) offset: i64,
}

pub(super) fn default_limit() -> i64 {
    100
}

#[derive(Deserialize, ToSchema)]
pub(super) struct CreateBotRequest {
    pub(super) account_id: Uuid,
    pub(super) name: String,
    pub(super) persona: String,
    pub(super) asset_focus: String,
    pub(super) algorithm: String,
    pub(super) strictness: String,
    pub(super) paper_mode: bool,
    pub(super) max_position_size_pct: f64,
    pub(super) max_daily_loss_pct: f64,
    pub(super) max_drawdown_pct: f64,
    pub(super) max_trades_per_day: i32,
    pub(super) llm_provider: String,
    pub(super) llm_api_key: String,
}

#[derive(Deserialize, ToSchema)]
pub(super) struct BotActionRequest {
    pub(super) action: String,
}

#[derive(Deserialize, ToSchema)]
pub(super) struct RegisterBotRequest {
    pub(super) bot_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub(super) struct AckConfigRequest {
    pub(super) config_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub(super) struct BotResponse {
    pub(super) id: Uuid,
    pub(super) account_id: Uuid,
    pub(super) name: String,
    pub(super) persona: String,
    pub(super) status: String,
    pub(super) droplet_id: Option<i64>,
    pub(super) desired_config_version_id: Option<Uuid>,
    pub(super) applied_config_version_id: Option<Uuid>,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
    pub(super) updated_at: chrono::DateTime<chrono::Utc>,
    #[schema(format = "date-time")]
    pub(super) last_heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
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
