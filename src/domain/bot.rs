use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bot {
    pub id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub persona: Persona,
    pub status: BotStatus,
    pub droplet_id: Option<i64>,
    pub desired_config_version_id: Option<Uuid>,
    pub applied_config_version_id: Option<Uuid>,
    pub registration_token: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Persona {
    Beginner,
    Tweaker,
    QuantLite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BotStatus {
    Pending,
    Provisioning,
    Online,
    Paused,
    Error,
    Destroyed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub id: Uuid,
    pub bot_id: Uuid,
    pub version: i32,
    pub trading_config: TradingConfig,
    pub risk_config: RiskConfig,
    pub secrets: BotSecrets,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredBotConfig {
    pub id: Uuid,
    pub bot_id: Uuid,
    pub version: i32,
    pub trading_config: TradingConfig,
    pub risk_config: RiskConfig,
    pub secrets: EncryptedBotSecrets,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    pub asset_focus: AssetFocus,
    pub algorithm: AlgorithmMode,
    pub strictness: StrictnessLevel,
    pub paper_mode: bool,
    pub signal_knobs: Option<SignalKnobs>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetFocus {
    Majors,
    Memes,
    Custom(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlgorithmMode {
    Trend,
    MeanReversion,
    Breakout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StrictnessLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalKnobs {
    pub volume_confirmation: bool,
    pub volatility_brake: bool,
    pub liquidity_filter: StrictnessLevel,
    pub correlation_brake: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub max_position_size_pct: f64,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    pub max_trades_per_day: i32,
}

impl RiskConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if !(0.0..=100.0).contains(&self.max_position_size_pct) {
            errors.push(format!(
                "max_position_size_pct must be between 0 and 100, got {}",
                self.max_position_size_pct
            ));
        }

        if !(0.0..=100.0).contains(&self.max_daily_loss_pct) {
            errors.push(format!(
                "max_daily_loss_pct must be between 0 and 100, got {}",
                self.max_daily_loss_pct
            ));
        }

        if !(0.0..=100.0).contains(&self.max_drawdown_pct) {
            errors.push(format!(
                "max_drawdown_pct must be between 0 and 100, got {}",
                self.max_drawdown_pct
            ));
        }

        if self.max_trades_per_day < 0 {
            errors.push(format!(
                "max_trades_per_day must be >= 0, got {}",
                self.max_trades_per_day
            ));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSecrets {
    pub llm_provider: String,
    pub llm_api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBotSecrets {
    pub llm_provider: String,
    #[serde(with = "serde_bytes")]
    pub llm_api_key_encrypted: Vec<u8>,
}

impl Bot {
    pub fn new(account_id: Uuid, name: String, persona: Persona) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            account_id,
            name,
            persona,
            status: BotStatus::Pending,
            droplet_id: None,
            desired_config_version_id: None,
            applied_config_version_id: None,
            registration_token: None,
            created_at: now,
            updated_at: now,
            last_heartbeat_at: None,
        }
    }
}
