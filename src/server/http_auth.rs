use axum::http::{header, header::HeaderMap};

pub(super) fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|t| !t.is_empty())
}

pub(super) fn is_admin_authorized(headers: &HeaderMap, expected_token: &str) -> bool {
    !expected_token.is_empty() && extract_bearer_token(headers) == Some(expected_token)
}
