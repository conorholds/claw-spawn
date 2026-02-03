use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Droplet {
    pub id: i64,
    pub name: String,
    pub region: String,
    pub size: String,
    pub image: String,
    pub status: DropletStatus,
    pub ip_address: Option<String>,
    pub bot_id: Option<uuid::Uuid>,
    pub created_at: DateTime<Utc>,
    pub destroyed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DropletStatus {
    New,
    Active,
    Off,
    Destroyed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropletCreateRequest {
    pub name: String,
    pub region: String,
    pub size: String,
    pub image: String,
    pub user_data: String,
    pub tags: Vec<String>,
}

impl Droplet {
    pub fn from_do_response(response: DigitalOceanDropletResponse) -> Self {
        Self {
            id: response.id,
            name: response.name,
            region: response.region.slug,
            size: response.size_slug,
            image: response.image.slug.unwrap_or_default(),
            status: DropletStatus::from_do_status(&response.status),
            ip_address: response.networks.v4.first().map(|n| n.ip_address.clone()),
            bot_id: None,
            created_at: Utc::now(),
            destroyed_at: None,
        }
    }
}

impl DropletStatus {
    fn from_do_status(status: &str) -> Self {
        match status {
            "new" => DropletStatus::New,
            "active" => DropletStatus::Active,
            "off" => DropletStatus::Off,
            _ => DropletStatus::Error,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DigitalOceanDropletResponse {
    pub id: i64,
    pub name: String,
    pub region: Region,
    pub size_slug: String,
    pub image: Image,
    pub status: String,
    pub networks: Networks,
}

#[derive(Debug, Deserialize)]
pub struct Region {
    pub slug: String,
}

#[derive(Debug, Deserialize)]
pub struct Image {
    pub slug: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Networks {
    pub v4: Vec<NetworkV4>,
}

#[derive(Debug, Deserialize)]
pub struct NetworkV4 {
    pub ip_address: String,
    pub type_: String,
}
