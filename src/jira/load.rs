//! 加载 JIRA 配置：default → TOML `[jira]` →（可选）GuiPrefs → env。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

use crate::jira::config::{JiraApiVersion, JiraAuthMode, JiraConfig};

/// 仅提取 TOML 中的 `[jira]` 段，避免强依赖完整 AppConfig。
#[derive(Debug, Default, Serialize, Deserialize)]
struct JiraTomlFile {
    #[serde(default)]
    jira: JiraConfig,
}

/// 可持久化的非 secret 子集（写入 GuiPrefs）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JiraPrefsSnapshot {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_version: Option<JiraApiVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<JiraAuthMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_screenshots: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_defect_zip: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_map: Option<HashMap<String, String>>,
}

impl JiraPrefsSnapshot {
    pub fn from_config(cfg: &JiraConfig) -> Self {
        Self {
            enabled: Some(cfg.enabled),
            base_url: cfg.base_url.clone(),
            project_key: cfg.project_key.clone(),
            issue_type: Some(cfg.issue_type.clone()),
            api_version: Some(cfg.api_version),
            auth_mode: Some(cfg.auth_mode),
            attach_screenshots: Some(cfg.attach_screenshots),
            attach_defect_zip: Some(cfg.attach_defect_zip),
            max_concurrent: Some(cfg.max_concurrent.clamp(1, 4)),
            labels: Some(cfg.labels.clone()),
            default_priority: Some(cfg.default_priority.clone()),
            priority_map: Some(cfg.priority_map.clone()),
        }
    }

    pub fn apply_to(&self, cfg: &mut JiraConfig) {
        if let Some(v) = self.enabled {
            cfg.enabled = v;
        }
        if let Some(ref url) = self.base_url {
            let url = url.trim();
            if !url.is_empty() {
                cfg.base_url.replace(url.to_string());
            }
        }
        if let Some(ref key) = self.project_key {
            let key = key.trim();
            if !key.is_empty() {
                cfg.project_key.replace(key.to_string());
            }
        }
        if let Some(ref issue_type) = self.issue_type {
            let issue_type = issue_type.trim();
            if !issue_type.is_empty() {
                cfg.issue_type = issue_type.to_string();
            }
        }
        if let Some(v) = self.api_version {
            cfg.api_version = v;
        }
        if let Some(v) = self.auth_mode {
            cfg.auth_mode = v;
        }
        if let Some(v) = self.attach_screenshots {
            cfg.attach_screenshots = v;
        }
        if let Some(v) = self.attach_defect_zip {
            cfg.attach_defect_zip = v;
        }
        if let Some(v) = self.max_concurrent {
            cfg.max_concurrent = v.clamp(1, 4);
        }
        if let Some(ref labels) = self.labels {
            cfg.labels = labels
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Some(ref priority) = self.default_priority {
            let priority = priority.trim();
            if !priority.is_empty() {
                cfg.default_priority = priority.to_string();
            }
        }
        if let Some(ref map) = self.priority_map {
            for (k, v) in map {
                let key = k.trim();
                let val = v.trim();
                if !key.is_empty() && !val.is_empty() {
                    cfg.priority_map.insert(key.to_string(), val.to_string());
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.base_url.is_none()
            && self.project_key.is_none()
            && self.issue_type.is_none()
            && self.api_version.is_none()
            && self.auth_mode.is_none()
            && self.attach_screenshots.is_none()
            && self.attach_defect_zip.is_none()
            && self.max_concurrent.is_none()
            && self.labels.is_none()
            && self.default_priority.is_none()
            && self.priority_map.is_none()
    }
}

/// 候选配置文件路径（cwd 下的 `imgforge.toml`，以及可选显式路径）。
pub fn default_toml_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let cwd = PathBuf::from("imgforge.toml");
    if cwd.exists() {
        out.push(cwd);
    }
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let p = PathBuf::from(home).join(".imgforge").join("imgforge.toml");
        if p.exists() {
            out.push(p);
        }
    }
    out
}

fn merge_toml_jira(path: &Path) -> Option<JiraConfig> {
    let figment = Figment::from(Serialized::defaults(JiraTomlFile::default()))
        .merge(Toml::file(path));
    figment.extract::<JiraTomlFile>().ok().map(|f| f.jira)
}

/// default → 第一个可读 TOML 的 `[jira]` → env。
pub fn load_jira_config() -> JiraConfig {
    load_jira_config_with_prefs(None)
}

/// default → TOML → prefs（优先于 TOML）→ env（最后覆盖）。
pub fn load_jira_config_with_prefs(prefs: Option<&JiraPrefsSnapshot>) -> JiraConfig {
    let mut cfg = JiraConfig::default();
    for path in default_toml_candidates() {
        if let Some(from_toml) = merge_toml_jira(&path) {
            cfg = from_toml;
            break;
        }
    }
    if let Some(prefs) = prefs {
        prefs.apply_to(&mut cfg);
    }
    cfg.apply_env_overrides();
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_apply_overrides_non_secret() {
        let mut cfg = JiraConfig::default();
        let prefs = JiraPrefsSnapshot {
            enabled: Some(true),
            base_url: Some("https://example.atlassian.net".into()),
            project_key: Some("CAM".into()),
            issue_type: Some("Bug".into()),
            api_version: Some(JiraApiVersion::V2),
            auth_mode: Some(JiraAuthMode::EnvBearer),
            attach_screenshots: Some(false),
            attach_defect_zip: Some(false),
            max_concurrent: Some(3),
            labels: Some(vec!["imgforge".into(), "review".into()]),
            default_priority: Some("High".into()),
            priority_map: Some(HashMap::from([("1".into(), "Critical".into())])),
        };
        prefs.apply_to(&mut cfg);
        assert!(cfg.enabled);
        assert_eq!(cfg.base_url.as_deref(), Some("https://example.atlassian.net"));
        assert_eq!(cfg.project_key.as_deref(), Some("CAM"));
        assert_eq!(cfg.api_version, JiraApiVersion::V2);
        assert!(!cfg.attach_screenshots);
        assert_eq!(cfg.max_concurrent, 3);
        assert_eq!(cfg.labels, vec!["imgforge", "review"]);
        assert_eq!(cfg.priority_map.get("1").map(String::as_str), Some("Critical"));
    }

    #[test]
    fn snapshot_roundtrip() {
        let cfg = JiraConfig {
            enabled: true,
            base_url: Some("https://x.atlassian.net".into()),
            project_key: Some("P".into()),
            attach_screenshots: false,
            max_concurrent: 2,
            labels: vec!["a".into(), "b".into()],
            priority_map: HashMap::from([
                ("1".into(), "Highest".into()),
                ("2".into(), "High".into()),
                ("3".into(), "Medium".into()),
                ("4".into(), "Low".into()),
                ("5".into(), "Lowest".into()),
            ]),
            ..JiraConfig::default()
        };
        let snap = JiraPrefsSnapshot::from_config(&cfg);
        let mut again = JiraConfig::default();
        snap.apply_to(&mut again);
        assert_eq!(again.enabled, cfg.enabled);
        assert_eq!(again.base_url, cfg.base_url);
        assert_eq!(again.project_key, cfg.project_key);
        assert_eq!(again.attach_screenshots, cfg.attach_screenshots);
        assert_eq!(again.max_concurrent, cfg.max_concurrent);
        assert_eq!(again.labels, cfg.labels);
        assert_eq!(again.priority_map.get("1"), cfg.priority_map.get("1"));
        assert_eq!(again.priority_map.get("5"), cfg.priority_map.get("5"));
    }

    #[test]
    fn max_concurrent_clamped_on_apply() {
        let mut cfg = JiraConfig::default();
        let prefs = JiraPrefsSnapshot {
            max_concurrent: Some(9),
            ..JiraPrefsSnapshot::default()
        };
        prefs.apply_to(&mut cfg);
        assert_eq!(cfg.max_concurrent, 4);
    }
}
