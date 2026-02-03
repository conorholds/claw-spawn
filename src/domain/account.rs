use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub id: Uuid,
    pub external_id: String,
    pub subscription_tier: SubscriptionTier,
    pub max_bots: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubscriptionTier {
    Free,
    Basic,
    Pro,
}

impl Account {
    pub fn new(external_id: String, tier: SubscriptionTier) -> Self {
        let now = Utc::now();
        let max_bots = match tier {
            SubscriptionTier::Free => 0,
            SubscriptionTier::Basic => 2,
            SubscriptionTier::Pro => 4,
        };

        Self {
            id: Uuid::new_v4(),
            external_id,
            subscription_tier: tier,
            max_bots,
            created_at: now,
            updated_at: now,
        }
    }
}
