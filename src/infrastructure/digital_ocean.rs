use crate::domain::{Droplet, DropletCreateRequest};
use reqwest::{header, Client};
use serde_json::json;
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;

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
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// REL-002: Retry configuration for DO API calls
const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Check if status code is retryable (500, 502, 503)
fn is_retryable_status(status: u16) -> bool {
    matches!(status, 500 | 502 | 503)
}

pub struct DigitalOceanClient {
    client: Client,
    base_url: String,
}

impl DigitalOceanClient {
    pub fn new(api_token: String) -> Result<Self, DigitalOceanError> {
        let mut headers = header::HeaderMap::new();
        let auth_value = match header::HeaderValue::from_str(&format!("Bearer {}", api_token)) {
            Ok(val) => val,
            Err(e) => {
                return Err(DigitalOceanError::InvalidConfig(format!(
                    "Invalid API token format: {}",
                    e
                )))
            }
        };
        headers.insert(header::AUTHORIZATION, auth_value);
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
            .map_err(|e| {
                DigitalOceanError::InvalidConfig(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Self {
            client,
            base_url: "https://api.digitalocean.com/v2".to_string(),
        })
    }

    async fn send_with_retry<F>(
        &self,
        mut request_builder: F,
        not_found_id: Option<i64>,
    ) -> Result<reqwest::Response, DigitalOceanError>
    where
        F: FnMut() -> reqwest::RequestBuilder,
    {
        let mut last_error: Option<String> = None;

        for attempt in 0..MAX_RETRIES {
            let response = request_builder().send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status().as_u16();

                    if status == 429 {
                        return Err(DigitalOceanError::RateLimited);
                    }

                    if status == 404 {
                        if let Some(id) = not_found_id {
                            return Err(DigitalOceanError::NotFound(id));
                        }
                    }

                    if is_retryable_status(status) && attempt < MAX_RETRIES - 1 {
                        let backoff = INITIAL_BACKOFF_MS * 2_u64.pow(attempt);
                        sleep(Duration::from_millis(backoff)).await;
                        continue;
                    }

                    return Ok(resp);
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                    if attempt < MAX_RETRIES - 1 {
                        let backoff = INITIAL_BACKOFF_MS * 2_u64.pow(attempt);
                        sleep(Duration::from_millis(backoff)).await;
                    }
                }
            }
        }

        Err(DigitalOceanError::RequestFailed(
            last_error.unwrap_or_else(|| "Max retries exceeded".to_string()),
        ))
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

        let resp = self
            .send_with_retry(
                || self.client.post(format!("{}/droplets", self.base_url)).json(&body),
                None,
            )
            .await?;

        if !resp.status().is_success() {
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::CreationFailed(error_text));
        }

        let json_response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        let droplet_data = json_response.get("droplet").ok_or_else(|| {
            DigitalOceanError::InvalidResponse("Missing droplet field".to_string())
        })?;

        let do_response: crate::domain::DigitalOceanDropletResponse =
            serde_json::from_value(droplet_data.clone())
                .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        Ok(Droplet::from_do_response(do_response))
    }

    pub async fn get_droplet(&self, droplet_id: i64) -> Result<Droplet, DigitalOceanError> {
        let resp = self
            .send_with_retry(
                || self
                    .client
                    .get(format!("{}/droplets/{}", self.base_url, droplet_id)),
                Some(droplet_id),
            )
            .await?;

        if !resp.status().is_success() {
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::RequestFailed(error_text));
        }

        let json_response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        let droplet_data = json_response.get("droplet").ok_or_else(|| {
            DigitalOceanError::InvalidResponse("Missing droplet field".to_string())
        })?;

        let do_response: crate::domain::DigitalOceanDropletResponse =
            serde_json::from_value(droplet_data.clone())
                .map_err(|e| DigitalOceanError::InvalidResponse(e.to_string()))?;

        Ok(Droplet::from_do_response(do_response))
    }

    pub async fn destroy_droplet(&self, droplet_id: i64) -> Result<(), DigitalOceanError> {
        let resp = self
            .send_with_retry(
                || self
                    .client
                    .delete(format!("{}/droplets/{}", self.base_url, droplet_id)),
                Some(droplet_id),
            )
            .await?;

        if !resp.status().is_success() {
            let error_text = resp
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

        let resp = self
            .send_with_retry(
                || {
                    self.client
                        .post(format!("{}/droplets/{}/actions", self.base_url, droplet_id))
                        .json(&body)
                },
                None,
            )
            .await?;

        if !resp.status().is_success() {
            let error_text = resp
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

        let resp = self
            .send_with_retry(
                || {
                    self.client
                        .post(format!("{}/droplets/{}/actions", self.base_url, droplet_id))
                        .json(&body)
                },
                None,
            )
            .await?;

        if !resp.status().is_success() {
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(DigitalOceanError::RequestFailed(error_text));
        }

        Ok(())
    }
}
