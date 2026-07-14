//! 远端接入配置：URL、工作区、认证模式与离线缓存策略。
//!
//! Token 不写入普通 TOML；通过环境变量或后续钥匙串读取。

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};

/// 认证模式（首期仅预留，不实现真实登录）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAuthMode {
    /// 未配置认证。
    #[default]
    None,
    /// 从环境变量读取 Bearer token（`IMGFORGE_REMOTE_TOKEN`）。
    EnvBearer,
    /// 预留：系统钥匙串 / 凭据管理器。
    Keychain,
}

impl RemoteAuthMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::EnvBearer => "env_bearer",
            Self::Keychain => "keychain",
        }
    }
}

/// 远端服务器客户端配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// 是否启用远端能力（默认关闭，本地路径不受影响）。
    #[serde(default)]
    pub enabled: bool,
    /// API 根地址，如 `https://api.example.com`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// 工作区 / 租户 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub auth_mode: RemoteAuthMode,
    /// 请求超时（秒）。
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// 是否启用离线任务缓存。
    #[serde(default = "default_true")]
    pub offline_cache: bool,
    /// 离线缓存文件路径；为空时使用默认应用数据目录。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<PathBuf>,
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: None,
            workspace_id: None,
            auth_mode: RemoteAuthMode::None,
            timeout_secs: default_timeout_secs(),
            offline_cache: true,
            cache_path: None,
        }
    }
}

impl RemoteConfig {
    /// 从环境变量叠加覆盖（不覆盖已有显式值以外的语义：仅填充空字段 / 开关）。
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("IMGFORGE_REMOTE_ENABLED") {
            self.enabled = matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        if let Ok(url) = std::env::var("IMGFORGE_REMOTE_BASE_URL") {
            let url = url.trim().to_string();
            if !url.is_empty() {
                self.base_url = Some(url);
            }
        }
        if let Ok(ws) = std::env::var("IMGFORGE_REMOTE_WORKSPACE_ID") {
            let ws = ws.trim().to_string();
            if !ws.is_empty() {
                self.workspace_id = Some(ws);
            }
        }
        if let Ok(mode) = std::env::var("IMGFORGE_REMOTE_AUTH_MODE") {
            self.auth_mode = match mode.trim().to_ascii_lowercase().as_str() {
                "env" | "env_bearer" | "bearer" => RemoteAuthMode::EnvBearer,
                "keychain" => RemoteAuthMode::Keychain,
                "none" | "" => RemoteAuthMode::None,
                _ => self.auth_mode,
            };
        }
        if let Ok(secs) = std::env::var("IMGFORGE_REMOTE_TIMEOUT_SECS") {
            if let Ok(n) = secs.trim().parse::<u64>() {
                if n > 0 {
                    self.timeout_secs = n;
                }
            }
        }
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.max(1))
    }

    /// 配置是否足以尝试连接（启用 + 有 base_url）。
    pub fn is_configured(&self) -> bool {
        self.enabled
            && self
                .base_url
                .as_ref()
                .map(|u| !u.trim().is_empty())
                .unwrap_or(false)
    }

    pub fn validate(&self) -> AppResult<()> {
        if !self.enabled {
            return Ok(());
        }
        let Some(url) = self.base_url.as_ref().map(|s| s.trim()) else {
            return Err(AppError::Config(
                "remote.enabled=true 但未设置 remote.base_url".into(),
            ));
        };
        if url.is_empty() {
            return Err(AppError::Config("remote.base_url 不能为空".into()));
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AppError::Config(
                "remote.base_url 必须以 http:// 或 https:// 开头".into(),
            ));
        }
        if self.timeout_secs == 0 {
            return Err(AppError::Config("remote.timeout_secs 必须 ≥ 1".into()));
        }
        Ok(())
    }

    /// 解析认证 token；失败时返回 None（不把密钥写入日志）。
    pub fn resolve_token(&self) -> Option<String> {
        match self.auth_mode {
            RemoteAuthMode::None => None,
            RemoteAuthMode::EnvBearer => std::env::var("IMGFORGE_REMOTE_TOKEN")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            RemoteAuthMode::Keychain => {
                // 预留：后续接入系统钥匙串。当前回退到环境变量，便于本地联调。
                std::env::var("IMGFORGE_REMOTE_TOKEN")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            }
        }
    }

    /// 默认离线缓存路径：`~/.imgforge/remote_jobs.toml`。
    pub fn resolved_cache_path(&self) -> PathBuf {
        if let Some(path) = &self.cache_path {
            return path.clone();
        }
        default_remote_cache_path()
    }

    pub fn status_label(&self) -> &'static str {
        if !self.enabled {
            "未启用"
        } else if self.is_configured() {
            "已配置"
        } else {
            "已启用但未配置 URL"
        }
    }
}

pub fn default_remote_cache_path() -> PathBuf {
    dirs_home()
        .map(|h| h.join(".imgforge").join("remote_jobs.toml"))
        .unwrap_or_else(|| PathBuf::from(".imgforge").join("remote_jobs.toml"))
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_validates() {
        let cfg = RemoteConfig::default();
        assert!(cfg.validate().is_ok());
        assert!(!cfg.is_configured());
    }

    #[test]
    fn enabled_without_url_fails() {
        let cfg = RemoteConfig {
            enabled: true,
            ..RemoteConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn enabled_with_https_ok() {
        let cfg = RemoteConfig {
            enabled: true,
            base_url: Some("https://api.example.com".into()),
            ..RemoteConfig::default()
        };
        assert!(cfg.validate().is_ok());
        assert!(cfg.is_configured());
    }
}
