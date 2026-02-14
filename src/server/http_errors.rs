use crate::application::{LifecycleError, ProvisioningError};
use crate::infrastructure::{DigitalOceanError, RepositoryError};
use axum::http::StatusCode;

pub(super) fn map_bot_action_error(err: &ProvisioningError) -> (StatusCode, serde_json::Value) {
    match err {
        ProvisioningError::InvalidConfig(msg) => {
            (StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg }))
        }
        ProvisioningError::Repository(RepositoryError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, serde_json::json!({ "error": "Bot not found" }))
        }
        ProvisioningError::DigitalOcean(DigitalOceanError::RateLimited) => (
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({ "error": "Rate limited by DigitalOcean, please retry" }),
        ),
        ProvisioningError::DigitalOcean(DigitalOceanError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Associated droplet not found" }),
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "Action failed" }),
        ),
    }
}

pub(super) fn map_create_bot_error(err: &ProvisioningError) -> (StatusCode, serde_json::Value) {
    match err {
        ProvisioningError::Repository(RepositoryError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Account not found" }),
        ),
        ProvisioningError::AccountLimitReached(max) => (
            StatusCode::FORBIDDEN,
            serde_json::json!({
                "error": format!("Account limit reached: maximum {} bots allowed", max)
            }),
        ),
        ProvisioningError::DigitalOcean(DigitalOceanError::RateLimited) => (
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({ "error": "Rate limited by DigitalOcean, please retry" }),
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "Failed to create bot" }),
        ),
    }
}

pub(super) fn map_bot_read_error(err: &LifecycleError) -> (StatusCode, serde_json::Value) {
    match err {
        LifecycleError::Repository(RepositoryError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, serde_json::json!({ "error": "Bot not found" }))
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "Failed to fetch bot" }),
        ),
    }
}

pub(super) fn map_bot_config_error(err: &LifecycleError) -> (StatusCode, serde_json::Value) {
    match err {
        LifecycleError::Repository(RepositoryError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Bot not found" }),
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "Failed to get config" }),
        ),
    }
}

pub(super) fn map_ack_config_error(err: &LifecycleError) -> (StatusCode, serde_json::Value) {
    match err {
        LifecycleError::Repository(RepositoryError::NotFound(_)) | LifecycleError::ConfigNotFound(_) => (
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Config not found" }),
        ),
        LifecycleError::ConfigVersionConflict { .. } => (
            StatusCode::CONFLICT,
            serde_json::json!({ "error": "Config version conflict" }),
        ),
        LifecycleError::InvalidState(_) => (
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "Invalid bot state for config acknowledgment" }),
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "Failed to acknowledge config" }),
        ),
    }
}

pub(super) fn map_account_read_error(err: &RepositoryError) -> (StatusCode, serde_json::Value) {
    match err {
        RepositoryError::NotFound(_) => {
            (StatusCode::NOT_FOUND, serde_json::json!({ "error": "Account not found" }))
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "Failed to get account" }),
        ),
    }
}
