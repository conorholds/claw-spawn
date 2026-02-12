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
        let ip_address = response
            .networks
            .v4
            .iter()
            .find(|n| n.type_ == "public")
            .map(|n| n.ip_address.clone());

        Self {
            id: response.id,
            name: response.name,
            region: response.region.slug,
            size: response.size_slug,
            image: response.image.slug.unwrap_or_default(),
            status: DropletStatus::from_do_status(&response.status),
            ip_address,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_do_response_prefers_public_ipv4() {
        let droplet = Droplet::from_do_response(DigitalOceanDropletResponse {
            id: 1,
            name: "d1".to_string(),
            region: Region {
                slug: "nyc3".to_string(),
            },
            size_slug: "s-1vcpu-2gb".to_string(),
            image: Image {
                slug: Some("ubuntu-22-04-x64".to_string()),
            },
            status: "active".to_string(),
            networks: Networks {
                v4: vec![
                    NetworkV4 {
                        ip_address: "10.0.0.5".to_string(),
                        type_: "private".to_string(),
                    },
                    NetworkV4 {
                        ip_address: "203.0.113.10".to_string(),
                        type_: "public".to_string(),
                    },
                ],
            },
        });

        assert_eq!(droplet.ip_address.as_deref(), Some("203.0.113.10"));
    }

    #[test]
    fn from_do_response_handles_missing_public_ipv4() {
        let droplet = Droplet::from_do_response(DigitalOceanDropletResponse {
            id: 1,
            name: "d1".to_string(),
            region: Region {
                slug: "nyc3".to_string(),
            },
            size_slug: "s-1vcpu-2gb".to_string(),
            image: Image { slug: None },
            status: "new".to_string(),
            networks: Networks {
                v4: vec![NetworkV4 {
                    ip_address: "10.0.0.5".to_string(),
                    type_: "private".to_string(),
                }],
            },
        });

        assert!(droplet.ip_address.is_none());
    }
}
