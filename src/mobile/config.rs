//! 移动设备拉取配置。

use std::path::PathBuf;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};

/// 移动设备拉取后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MobilePullBackend {
    /// 自动选择：优先本地挂载目录，失败后尝试 ADB。
    #[default]
    Auto,
    /// 从已挂载为本地目录的设备路径复制。
    Fs,
    /// 通过 Android Debug Bridge 拉取。
    Adb,
}

/// ADB 二进制选择策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AdbBinaryMode {
    /// 优先自定义路径，再使用内置 ADB，最后回退 PATH。
    #[default]
    Auto,
    /// 只使用发布包内置 ADB。
    Bundled,
    /// 只使用 `adb_path`。
    Custom,
    /// 只使用 PATH 中的 adb。
    Path,
}

/// 移动设备拉取配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobilePullConfig {
    /// 是否启用移动设备拉取。
    #[serde(default)]
    pub enabled: bool,
    /// 拉取后端。
    #[serde(default)]
    pub backend: MobilePullBackend,
    /// 设备端来源路径。`fs` 为本地目录；`adb` 为 Android 设备路径。
    #[serde(default = "default_source_path")]
    pub source_path: String,
    /// 本地暂存目录。拉取完成后转换流程会从这里读取输入。
    #[serde(default = "default_staging_dir")]
    pub staging_dir: PathBuf,
    /// 是否保留源目录结构。
    #[serde(default = "default_true")]
    pub preserve_structure: bool,
    /// 多设备连接时指定 ADB serial。
    #[serde(default)]
    pub adb_serial: Option<String>,
    /// ADB 二进制选择策略。
    #[serde(default)]
    pub adb_mode: AdbBinaryMode,
    /// 自定义 ADB 路径。
    #[serde(default)]
    pub adb_path: Option<PathBuf>,
    /// 是否允许回退到系统 PATH 中的 ADB。
    #[serde(default = "default_true")]
    pub allow_path_fallback: bool,
    /// 拉取后删除设备端文件。第一版不执行删除，保留配置位防止未来破坏性默认。
    #[serde(default)]
    pub delete_after_pull: bool,
}

impl Default for MobilePullConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: MobilePullBackend::default(),
            source_path: default_source_path(),
            staging_dir: default_staging_dir(),
            preserve_structure: true,
            adb_serial: None,
            adb_mode: AdbBinaryMode::default(),
            adb_path: None,
            allow_path_fallback: true,
            delete_after_pull: false,
        }
    }
}

impl MobilePullConfig {
    pub fn validate(&self) -> AppResult<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.source_path.trim().is_empty() {
            return Err(AppError::Config(
                "mobile pull source path cannot be empty".into(),
            ));
        }
        if self.staging_dir.as_os_str().is_empty() {
            return Err(AppError::Config(
                "mobile pull staging directory cannot be empty".into(),
            ));
        }
        if self.delete_after_pull {
            return Err(AppError::Config(
                "mobile delete-after-pull is not implemented yet".into(),
            ));
        }
        if self.adb_mode == AdbBinaryMode::Custom && self.adb_path.is_none() {
            return Err(AppError::Config(
                "mobile adb mode 'custom' requires mobile.adb_path".into(),
            ));
        }
        Ok(())
    }
}

fn default_source_path() -> String {
    "/sdcard/DCIM".to_string()
}

fn default_staging_dir() -> PathBuf {
    PathBuf::from(".imgforge/mobile-import")
}

fn default_true() -> bool {
    true
}
