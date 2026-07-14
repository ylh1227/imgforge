//! 配置模块：结构体与预设。

pub mod app_config;
pub mod presets;
pub mod project_config;

pub use crate::mobile::{AdbBinaryMode, MobilePullBackend, MobilePullConfig};
pub use crate::remote::config::{RemoteAuthMode, RemoteConfig};
pub use app_config::{AppConfig, ConvertOverride};
pub use project_config::{post_webhook_json, ProjectConfig, ProjectExportTemplate, WebhookConfig};
