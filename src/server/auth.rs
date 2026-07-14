//! Lightweight API auth and workspace helpers.

use axum::http::HeaderMap;

use crate::server::api::{request_id_from_headers, ApiError};
use crate::server::config::ServerConfig;
use crate::server::storage::StoreError;

pub const WORKSPACE_HEADER: &str = "x-imgforge-workspace";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub token_present: bool,
    pub workspace_id: String,
    pub actor: Option<String>,
}

pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let trimmed = value.trim();
            trimmed
                .strip_prefix("Bearer ")
                .or_else(|| trimmed.strip_prefix("bearer "))
        })
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

pub fn authorize(config: &ServerConfig, headers: &HeaderMap) -> Result<AuthContext, ApiError> {
    let request_id = request_id_from_headers(headers);
    let bearer = extract_bearer(headers);
    if let Some(expected) = config.auth_token.as_deref() {
        if bearer.as_deref() != Some(expected) {
            return Err(ApiError::unauthorized(request_id));
        }
    }

    let workspace_id = headers
        .get(WORKSPACE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&config.default_workspace)
        .to_string();

    Ok(AuthContext {
        token_present: bearer.is_some(),
        workspace_id,
        actor: bearer.as_ref().map(|_| "bearer-token".to_string()),
    })
}

pub fn enforce_workspace(
    expected: &str,
    actual: Option<&str>,
    request_id: impl Into<String>,
) -> Result<(), ApiError> {
    enforce_workspace_store(expected, actual).map_err(|_| {
        ApiError::forbidden(
            request_id,
            format!(
                "workspace mismatch: expected {expected}, got {}",
                actual.unwrap_or("<empty>")
            ),
        )
    })
}

pub fn enforce_workspace_store(expected: &str, actual: Option<&str>) -> Result<(), StoreError> {
    let Some(actual) = actual.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if actual == expected {
        Ok(())
    } else {
        Err(StoreError::Validation(format!(
            "workspace mismatch: expected {expected}, got {actual}"
        )))
    }
}
