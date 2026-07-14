//! 远端接入错误类型。

use thiserror::Error;

use crate::core::error::AppError;
use crate::remote::contract::{RemoteApiErrorBody, RemoteApiErrorCode};

#[derive(Debug, Error)]
pub enum RemoteError {
    #[error("remote is disabled")]
    Disabled,

    #[error("remote is not configured: {0}")]
    NotConfigured(String),

    #[error("remote authentication required")]
    AuthRequired,

    #[error("remote request failed: {0}")]
    Request(String),

    #[error("remote API error [{code}]: {message}")]
    Api {
        code: RemoteApiErrorCode,
        message: String,
        retryable: bool,
        details: Option<String>,
        request_id: Option<String>,
    },

    #[error("remote job not found: {0}")]
    JobNotFound(String),

    #[error("remote cache error: {0}")]
    Cache(String),

    #[error("unsupported remote schema version: {0}")]
    UnsupportedSchema(u32),

    #[error("{0}")]
    Other(String),
}

pub type RemoteResult<T> = Result<T, RemoteError>;

impl RemoteError {
    pub fn from_api_body(body: RemoteApiErrorBody) -> Self {
        match body.code {
            RemoteApiErrorCode::Unauthorized | RemoteApiErrorCode::Forbidden => Self::AuthRequired,
            RemoteApiErrorCode::NotFound => Self::JobNotFound(body.message),
            RemoteApiErrorCode::UnsupportedSchema => {
                // message 里可能带版本号；无法解析时用 0。
                let ver = body
                    .details
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                Self::UnsupportedSchema(ver)
            }
            _ => Self::Api {
                code: body.code,
                message: body.message,
                retryable: body.retryable,
                details: body.details,
                request_id: body.request_id,
            },
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api { retryable, .. } => *retryable,
            Self::Request(msg) => {
                let lower = msg.to_ascii_lowercase();
                lower.contains("429")
                    || lower.contains("502")
                    || lower.contains("503")
                    || lower.contains("504")
                    || lower.contains("timeout")
                    || lower.contains("timed out")
            }
            _ => false,
        }
    }
}

impl From<RemoteError> for AppError {
    fn from(value: RemoteError) -> Self {
        AppError::Other(value.to_string())
    }
}
