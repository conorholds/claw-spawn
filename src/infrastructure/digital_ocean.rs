use crate::domain::{Droplet, DropletCreateRequest};
use reqwest::{Client, header};
use serde_json::json;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DigitalOceanError {
    #[error("API request failed: {0}")]
    RequestFailed(String),
    #[error("Droplet creation failed: {0}")]
    CreationFailed(String),
    #[error("Droplet not found: {0}")]
    NotFound(i64),
    #[error("Rate limited")]
    RateLimited,
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

pub struct DigitalOceanClient {
    client: Client,
    #[allow(dead_code)]
    api_token: String,
    base_url: String,
}

impl DigitalOceanClient {
    pub fn new(api_token: String) -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", api_token).parse().unwrap(),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        let client = Client::builder()
            .default_headers(headers)
            // CRIT-004: Add timeouts to prevent hanging requests
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            api_token,
            base_url: "https://api.digitalocean.com/v2".to_string(),
        }
    }

    pub async fn create_droplet(
        &self,
        request: DropletCreateRequest,
    ) -> Result<Droplet, DigitalOceanError> {
        let body = json!({
            "name": request.name,
            "region": request.region,
            "size": request.size,
            "image": request.image,
            "user_data": request.user_data,
            "tags": request.tags,
            "monitoring": true,
            "ipv6": false,
            "backups": false,
        });

        let response = self
            .client
            .post(format!("{}/droplets", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| DigitalOceanError::RequestFailed(e.to_string()))?;

        if response.status().as_u16() == 429 {
            return Err(DigitalOceanError::RateLimited);
        }

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::CreationFailed(error_text));
        }

        let json_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        let droplet_data = json_response
            .get("droplet")
            .ok_or_else(|| DigitalOceanError::InvalidResponse("Missing droplet field".to_string()))?;

        let do_response: crate::domain::DigitalOceanDropletResponse =
            serde_json::from_value(droplet_data.clone())
                .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        Ok(Droplet::from_do_response(do_response))
    }

    pub async fn get_droplet(&self, droplet_id: i64) -> Result<Droplet, DigitalOceanError> {
        let response = self
            .client
            .get(format!("{}/droplets/{}", self.base_url, droplet_id))
            .send()
            .await
            .map_err(|e| DigitalOceanError::RequestFailed(e.to_string()))?;

        if response.status().as_u16() == 429 {
            return Err(DigitalOceanError::RateLimited);
        }

        if response.status().as_u16() == 404 {
            return Err(DigitalOceanError::NotFound(droplet_id));
        }

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::RequestFailed(error_text));
        }

        let json_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        let droplet_data = json_response
            .get("droplet")
            .ok_or_else(|| DigitalOceanError::InvalidResponse("Missing droplet field".to_string()))?;

        let do_response: crate::domain::DigitalOceanDropletResponse =
            serde_json::from_value(droplet_data.clone())
                .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        Ok(Droplet::from_do_response(do_response))
    }

    pub async fn destroy_droplet(&self, droplet_id: i64) -> Result<(), DigitalOceanError> {
        let response = self
            .client
            .delete(format!("{}/droplets/{}", self.base_url, droplet_id))
            .send()
            .await
            .map_err(|e| DigitalOceanError::RequestFailed(e.to_string()))?;

        if response.status().as_u16() == 429 {
            return Err(DigitalOceanError::RateLimited);
        }

        if response.status().as_u16() == 404 {
            return Err(DigitalOceanError::NotFound(droplet_id));
        }

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::RequestFailed(error_text));
        }

        Ok(())
    }

    pub async fn shutdown_droplet(&self, droplet_id: i64) -> Result<(), DigitalOceanError> {
        let body = json!({
            "type": "shutdown",
        });

        let response = self
            .client
            .post(format!("{}/droplets/{}/actions", self.base_url, droplet_id))
            .json(&body)
            .send()
            .await
            .map_err(|e| DigitalOceanError::RequestFailed(e.to_string()))?;

        if response.status().as_u16() == 429 {
            return Err(DigitalOceanError::RateLimited);
        }

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::RequestFailed(error_text));
        }

        Ok(())
    }

    pub async fn reboot_droplet(&self, droplet_id: i64) -> Result<(), DigitalOceanError> {
        let body = json!({
            "type": "reboot",
        });

        let response = self
            .client
            .post(format!("{}/droplets/{}/actions", self.base_url, droplet_id))
            .json(&body)
            .send()
            .await
            .map_err(|e| DigitalOceanError::RequestFailed(e.to_string()))?;

        if response.status().as_u16() == 429 {
            return Err(DigitalOceanError::RateLimited);
        }

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::RequestFailed(error_text));
        }

        Ok(())
    }
}
