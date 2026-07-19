//! JIRA 客户端错误类型。

use thiserror::Error;

pub type JiraResult<T> = Result<T, JiraError>;

#[derive(Debug, Error)]
pub enum JiraError {
    #[error("JIRA 未启用（jira.enabled=false）")]
    Disabled,

    #[error("JIRA 未配置：{0}")]
    NotConfigured(String),

    #[error("JIRA 认证失败：{0}")]
    Auth(String),

    #[error("JIRA HTTP {status}: {message}")]
    Api { status: u16, message: String },

    #[error("JIRA 网络错误：{0}")]
    Network(String),

    #[error("JIRA 响应解析失败：{0}")]
    Parse(String),

    #[error("附件过大（{size} > {limit} bytes）：{path}")]
    AttachmentTooLarge {
        path: String,
        size: u64,
        limit: u64,
    },

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl JiraError {
    pub fn is_auth_failure(&self) -> bool {
        matches!(self, Self::Auth(_))
            || matches!(self, Self::Api { status, .. } if *status == 401 || *status == 403)
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api { status, .. } => *status == 429 || (*status >= 500 && *status <= 599),
            Self::Network(_) => true,
            _ => false,
        }
    }
}
