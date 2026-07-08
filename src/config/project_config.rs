//! 项目级配置：随素材目录移动的规则、模板和集成默认值。

use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: Option<String>,
    #[serde(default)]
    pub default_tags: Vec<String>,
    #[serde(default)]
    pub marker_templates: Vec<String>,
    #[serde(default)]
    pub export_templates: Vec<ProjectExportTemplate>,
    #[serde(default)]
    pub webhook: Option<WebhookConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectExportTemplate {
    pub module: String,
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
}

impl ProjectConfig {
    pub fn path_for_root(root: &Path) -> PathBuf {
        root.join(".imgforge").join("project.toml")
    }

    pub fn load_from_root(root: &Path) -> std::io::Result<Self> {
        let path = Self::path_for_root(root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        toml::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn save_to_root(&self, root: &Path) -> std::io::Result<()> {
        let path = Self::path_for_root(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, raw)
    }
}

pub fn post_webhook_json(config: &WebhookConfig, payload: &str) -> std::io::Result<()> {
    if !config.enabled {
        return Ok(());
    }
    let Some(rest) = config.url.strip_prefix("http://") else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Webhook 第一版仅支持 http:// URL",
        ));
    };
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut stream = TcpStream::connect(host_port)?;
    let path = format!("/{path}");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.as_bytes().len(),
        payload
    );
    stream.write_all(request.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ProjectConfig {
            name: Some("demo".into()),
            default_tags: vec!["黑场".into()],
            marker_templates: vec!["字幕错误".into()],
            export_templates: vec![ProjectExportTemplate {
                module: "数据提取".into(),
                name: "报告".into(),
                columns: vec!["batch".into()],
            }],
            webhook: None,
        };
        cfg.save_to_root(dir.path()).unwrap();
        let loaded = ProjectConfig::load_from_root(dir.path()).unwrap();
        assert_eq!(loaded.name.as_deref(), Some("demo"));
        assert_eq!(loaded.default_tags, vec!["黑场"]);
    }
}
