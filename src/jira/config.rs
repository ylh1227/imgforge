//! JIRA 接入配置：URL、项目、认证模式与字段映射。
//!
//! Token / PAT 不写入普通 TOML；通过环境变量读取。

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::error::{AppError, AppResult};

/// 认证模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JiraAuthMode {
    /// Cloud：邮箱 + API Token（Basic）。
    #[default]
    EnvBasic,
    /// Server/DC：PAT（Bearer）。
    EnvBearer,
}

impl JiraAuthMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::EnvBasic => "env_basic",
            Self::EnvBearer => "env_bearer",
        }
    }
}

/// REST API 主版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum JiraApiVersion {
    /// Jira Cloud 常用。
    #[default]
    #[serde(rename = "3", alias = "v3")]
    V3,
    /// Server / Data Center 常用。
    #[serde(rename = "2", alias = "v2")]
    V2,
}

impl JiraApiVersion {
    pub fn path_segment(self) -> &'static str {
        match self {
            Self::V3 => "3",
            Self::V2 => "2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::V3 => "3",
            Self::V2 => "2",
        }
    }
}

fn default_issue_type() -> String {
    "Bug".into()
}

fn default_timeout_secs() -> u64 {
    60
}

fn default_max_concurrent() -> u32 {
    1
}

fn default_true() -> bool {
    true
}

fn default_max_attach_bytes() -> u64 {
    50 * 1024 * 1024
}

fn default_priority_name() -> String {
    "Medium".into()
}

fn default_labels() -> Vec<String> {
    vec!["imgforge".into()]
}

fn default_priority_map() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("1".into(), "Highest".into());
    m.insert("2".into(), "High".into());
    m.insert("3".into(), "Medium".into());
    m.insert("4".into(), "Low".into());
    m.insert("5".into(), "Lowest".into());
    m
}

/// JIRA 客户端配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JiraConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_key: Option<String>,
    #[serde(default = "default_issue_type")]
    pub issue_type: String,
    #[serde(default)]
    pub api_version: JiraApiVersion,
    #[serde(default)]
    pub auth_mode: JiraAuthMode,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    #[serde(default = "default_true")]
    pub attach_screenshots: bool,
    #[serde(default = "default_true")]
    pub attach_defect_zip: bool,
    #[serde(default = "default_max_attach_bytes")]
    pub max_attach_bytes: u64,
    #[serde(default = "default_priority_name")]
    pub default_priority: String,
    #[serde(default = "default_priority_map")]
    pub priority_map: HashMap<String, String>,
    #[serde(default = "default_labels")]
    pub labels: Vec<String>,
    /// 高级用户可注入额外 fields（JSON 对象，合并进 create issue）。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra_fields: HashMap<String, Value>,
}

impl Default for JiraConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: None,
            project_key: None,
            issue_type: default_issue_type(),
            api_version: JiraApiVersion::default(),
            auth_mode: JiraAuthMode::default(),
            timeout_secs: default_timeout_secs(),
            max_concurrent: default_max_concurrent(),
            attach_screenshots: true,
            attach_defect_zip: true,
            max_attach_bytes: default_max_attach_bytes(),
            default_priority: default_priority_name(),
            priority_map: default_priority_map(),
            labels: default_labels(),
            extra_fields: HashMap::new(),
        }
    }
}

impl JiraConfig {
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("IMGFORGE_JIRA_ENABLED") {
            self.enabled = matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        if let Ok(url) = std::env::var("IMGFORGE_JIRA_BASE_URL") {
            let url = url.trim().to_string();
            if !url.is_empty() {
                self.base_url = Some(url);
            }
        }
        if let Ok(key) = std::env::var("IMGFORGE_JIRA_PROJECT_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                self.project_key = Some(key);
            }
        }
        if let Ok(issue_type) = std::env::var("IMGFORGE_JIRA_ISSUE_TYPE") {
            let issue_type = issue_type.trim().to_string();
            if !issue_type.is_empty() {
                self.issue_type = issue_type;
            }
        }
        if let Ok(mode) = std::env::var("IMGFORGE_JIRA_AUTH_MODE") {
            self.auth_mode = match mode.trim().to_ascii_lowercase().as_str() {
                "env_bearer" | "bearer" | "pat" => JiraAuthMode::EnvBearer,
                "env_basic" | "basic" | "" => JiraAuthMode::EnvBasic,
                _ => self.auth_mode,
            };
        }
        if let Ok(ver) = std::env::var("IMGFORGE_JIRA_API_VERSION") {
            self.api_version = match ver.trim() {
                "2" | "v2" => JiraApiVersion::V2,
                "3" | "v3" => JiraApiVersion::V3,
                _ => self.api_version,
            };
        }
        if let Ok(secs) = std::env::var("IMGFORGE_JIRA_TIMEOUT_SECS") {
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

    /// 建单并发度，限制在 1..=4。
    pub fn effective_concurrency(&self) -> usize {
        self.max_concurrent.clamp(1, 4) as usize
    }

    pub fn is_configured(&self) -> bool {
        self.enabled
            && self
                .base_url
                .as_ref()
                .map(|u| !u.trim().is_empty())
                .unwrap_or(false)
            && self
                .project_key
                .as_ref()
                .map(|k| !k.trim().is_empty())
                .unwrap_or(false)
    }

    pub fn validate(&self) -> AppResult<()> {
        if !self.enabled {
            return Ok(());
        }
        let Some(url) = self.base_url.as_ref().map(|s| s.trim()) else {
            return Err(AppError::Config(
                "jira.enabled=true 但未设置 jira.base_url".into(),
            ));
        };
        if url.is_empty() {
            return Err(AppError::Config("jira.base_url 不能为空".into()));
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AppError::Config(
                "jira.base_url 必须以 http:// 或 https:// 开头".into(),
            ));
        }
        let Some(key) = self.project_key.as_ref().map(|s| s.trim()) else {
            return Err(AppError::Config(
                "jira.enabled=true 但未设置 jira.project_key".into(),
            ));
        };
        if key.is_empty() {
            return Err(AppError::Config("jira.project_key 不能为空".into()));
        }
        if self.issue_type.trim().is_empty() {
            return Err(AppError::Config("jira.issue_type 不能为空".into()));
        }
        if self.timeout_secs == 0 {
            return Err(AppError::Config("jira.timeout_secs 必须 ≥ 1".into()));
        }
        if self.max_concurrent == 0 {
            return Err(AppError::Config("jira.max_concurrent 必须 ≥ 1".into()));
        }
        if self.max_concurrent > 4 {
            return Err(AppError::Config(
                "jira.max_concurrent 最大为 4（避免触发 JIRA 限流）".into(),
            ));
        }
        Ok(())
    }

    /// Cloud Basic：email + API token。
    pub fn resolve_basic_credentials(&self) -> Option<(String, String)> {
        if self.auth_mode != JiraAuthMode::EnvBasic {
            return None;
        }
        let email = std::env::var("IMGFORGE_JIRA_EMAIL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        let token = std::env::var("IMGFORGE_JIRA_API_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        Some((email, token))
    }

    /// Server/DC Bearer PAT。
    pub fn resolve_bearer_token(&self) -> Option<String> {
        if self.auth_mode != JiraAuthMode::EnvBearer {
            return None;
        }
        std::env::var("IMGFORGE_JIRA_PAT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn has_credentials(&self) -> bool {
        match self.auth_mode {
            JiraAuthMode::EnvBasic => self.resolve_basic_credentials().is_some(),
            JiraAuthMode::EnvBearer => self.resolve_bearer_token().is_some(),
        }
    }

    pub fn priority_for_severity(&self, severity: u8) -> String {
        self.priority_map
            .get(&severity.to_string())
            .cloned()
            .unwrap_or_else(|| self.default_priority.clone())
    }

    pub fn issue_browse_url(&self, issue_key: &str) -> Option<String> {
        let base = self.base_url.as_ref()?.trim().trim_end_matches('/');
        if base.is_empty() || issue_key.is_empty() {
            return None;
        }
        Some(format!("{base}/browse/{issue_key}"))
    }

    pub fn status_label(&self) -> &'static str {
        if !self.enabled {
            "未启用"
        } else if self.is_configured() {
            "已配置"
        } else {
            "已启用但未配置完整"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_validates() {
        let cfg = JiraConfig::default();
        assert!(cfg.validate().is_ok());
        assert!(!cfg.is_configured());
    }

    #[test]
    fn enabled_without_url_fails() {
        let cfg = JiraConfig {
            enabled: true,
            project_key: Some("CAM".into()),
            ..JiraConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn enabled_with_https_and_project_ok() {
        let cfg = JiraConfig {
            enabled: true,
            base_url: Some("https://example.atlassian.net".into()),
            project_key: Some("CAM".into()),
            ..JiraConfig::default()
        };
        assert!(cfg.validate().is_ok());
        assert!(cfg.is_configured());
        assert_eq!(
            cfg.issue_browse_url("CAM-1").as_deref(),
            Some("https://example.atlassian.net/browse/CAM-1")
        );
    }

    #[test]
    fn priority_map_defaults() {
        let cfg = JiraConfig::default();
        assert_eq!(cfg.priority_for_severity(1), "Highest");
        assert_eq!(cfg.priority_for_severity(3), "Medium");
        assert_eq!(cfg.priority_for_severity(9), "Medium");
    }

    #[test]
    fn max_concurrent_clamped_and_validated() {
        let cfg = JiraConfig {
            max_concurrent: 9,
            ..JiraConfig::default()
        };
        assert_eq!(cfg.effective_concurrency(), 4);

        let bad = JiraConfig {
            enabled: true,
            base_url: Some("https://example.atlassian.net".into()),
            project_key: Some("CAM".into()),
            max_concurrent: 5,
            ..JiraConfig::default()
        };
        assert!(bad.validate().is_err());
    }
}
