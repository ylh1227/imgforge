//! 远端 API 统一信封、错误体与控制面契约（schema v1）。

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::remote::types::{now_unix, REMOTE_SCHEMA_VERSION};

/// 统一 API 错误码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteApiErrorCode {
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Validation,
    RateLimited,
    Unavailable,
    UnsupportedSchema,
    Internal,
    Other,
}

impl RemoteApiErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Validation => "validation",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::Internal => "internal",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for RemoteApiErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 服务端统一错误响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteApiErrorBody {
    pub code: RemoteApiErrorCode,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl RemoteApiErrorBody {
    pub fn new(code: RemoteApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: matches!(
                code,
                RemoteApiErrorCode::RateLimited | RemoteApiErrorCode::Unavailable
            ),
            details: None,
            request_id: None,
        }
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }
}

/// 成功响应可选元数据信封字段（可嵌在具体 payload 旁，或作为 wrapper）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteResponseMeta {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub server_time: u64,
}

impl RemoteResponseMeta {
    pub fn new(request_id: Option<String>) -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            request_id,
            server_time: now_unix(),
        }
    }
}

/// 带元数据的通用成功包装。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteEnvelope<T> {
    #[serde(flatten)]
    pub meta: RemoteResponseMeta,
    pub data: T,
}

impl<T> RemoteEnvelope<T> {
    pub fn new(data: T, request_id: Option<String>) -> Self {
        Self {
            meta: RemoteResponseMeta::new(request_id),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_body_marks_rate_limit_retryable() {
        let err = RemoteApiErrorBody::new(RemoteApiErrorCode::RateLimited, "slow down");
        assert!(err.retryable);
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("rate_limited"));
    }

    #[test]
    fn envelope_includes_schema_version() {
        let env = RemoteEnvelope::new(42_u32, Some("req-1".into()));
        assert_eq!(env.meta.schema_version, REMOTE_SCHEMA_VERSION);
        assert_eq!(env.meta.request_id.as_deref(), Some("req-1"));
    }
}
