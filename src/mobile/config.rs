//! 移动设备拉取配置。

use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};

/// 拉取并发合法范围。
pub const MOBILE_PULL_CONCURRENCY_MIN: usize = 1;
pub const MOBILE_PULL_CONCURRENCY_MAX: usize = 8;
pub const MOBILE_PULL_CONCURRENCY_DEFAULT: usize = 4;

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

/// 单台设备的拉取目标（serial + 可选独立来源/保存路径）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdbDevicePull {
    pub serial: String,
    /// 该设备上的来源路径；省略或空则使用全局 `source_path`。
    #[serde(default)]
    pub source_path: Option<String>,
    /// 该设备本地保存目录；省略或空则使用全局 `staging_dir/<serial>/`。
    #[serde(default)]
    pub staging_dir: Option<PathBuf>,
}

/// 解析后的单设备拉取目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDeviceTarget {
    pub serial: String,
    pub source_path: String,
    /// 已解析的本地保存根目录（含「共用暂存 + serial 子目录」或「设备专属路径」）。
    pub staging_root: PathBuf,
}

impl AdbDevicePull {
    pub fn new(serial: impl Into<String>, source_path: impl Into<String>) -> Self {
        Self::with_paths(serial, source_path, None::<PathBuf>)
    }

    pub fn with_paths(
        serial: impl Into<String>,
        source_path: impl Into<String>,
        staging_dir: Option<impl Into<PathBuf>>,
    ) -> Self {
        let path = source_path.into();
        let staging = staging_dir.map(Into::into).and_then(|p: PathBuf| {
            if p.as_os_str().is_empty() {
                None
            } else {
                Some(p)
            }
        });
        Self {
            serial: serial.into(),
            source_path: if path.trim().is_empty() {
                None
            } else {
                Some(path)
            },
            staging_dir: staging,
        }
    }

    pub fn resolved_source<'a>(&'a self, default_source: &'a str) -> &'a str {
        self.source_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default_source)
    }

    pub fn staging_override(&self) -> Option<&PathBuf> {
        self.staging_dir
            .as_ref()
            .filter(|p| !p.as_os_str().is_empty())
    }
}

/// 将 serial 转为安全目录名（用于共用暂存下的子文件夹）。
pub fn sanitize_serial(serial: &str) -> String {
    let mut out = String::with_capacity(serial.len());
    for c in serial.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "device".into()
    } else {
        out
    }
}

/// 解析单台设备的本地保存根目录。
///
/// - 若该设备指定了 `staging_dir` → 直接使用该路径
/// - 否则 → `default_staging / <sanitize(serial)>`
pub fn resolve_device_staging_root(
    serial: &str,
    staging_override: Option<&PathBuf>,
    default_staging: &std::path::Path,
) -> PathBuf {
    match staging_override {
        Some(p) => p.clone(),
        None => default_staging.join(sanitize_serial(serial)),
    }
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
    /// 默认设备端来源路径。`fs` 为本地目录；`adb` 为 Android 设备路径。
    /// 按设备覆盖见 `adb_devices`。
    #[serde(default = "default_source_path")]
    pub source_path: String,
    /// 本地暂存目录。拉取完成后转换流程会从这里读取输入。
    #[serde(default = "default_staging_dir")]
    pub staging_dir: PathBuf,
    /// 是否保留源目录结构。
    #[serde(default = "default_true")]
    pub preserve_structure: bool,
    /// 多设备连接时指定 ADB serial（兼容单值）。
    #[serde(default)]
    pub adb_serial: Option<String>,
    /// 多台设备 serial 列表；非空时优先于 `adb_serial`。空 = 拉全部已授权设备。
    #[serde(default)]
    pub adb_serials: Vec<String>,
    /// 按设备指定 serial 与来源路径；非空时优先于 `adb_serials` / `adb_serial`。
    #[serde(default)]
    pub adb_devices: Vec<AdbDevicePull>,
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
    /// 单设备内按文件并发拉取数（1–8）。
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
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
            adb_serials: Vec::new(),
            adb_devices: Vec::new(),
            adb_mode: AdbBinaryMode::default(),
            adb_path: None,
            allow_path_fallback: true,
            delete_after_pull: false,
            concurrency: default_concurrency(),
        }
    }
}

impl MobilePullConfig {
    pub fn validate(&self) -> AppResult<()> {
        if !self.enabled {
            return Ok(());
        }
        let default_src = self.source_path.trim();
        let default_staging_empty = self.staging_dir.as_os_str().is_empty();
        if self.adb_devices.is_empty() {
            if default_src.is_empty() {
                return Err(AppError::Config(
                    "mobile pull source path cannot be empty".into(),
                ));
            }
            if default_staging_empty {
                return Err(AppError::Config(
                    "mobile pull staging directory cannot be empty".into(),
                ));
            }
        } else {
            for device in &self.adb_devices {
                if device.serial.trim().is_empty() {
                    return Err(AppError::Config(
                        "mobile adb_devices entry serial cannot be empty".into(),
                    ));
                }
                if device.resolved_source(default_src).is_empty() {
                    return Err(AppError::Config(format!(
                        "mobile pull source path for device '{}' cannot be empty",
                        device.serial.trim()
                    )));
                }
                if device.staging_override().is_none() && default_staging_empty {
                    return Err(AppError::Config(format!(
                        "device '{}' has no staging_dir and global staging_dir is empty",
                        device.serial.trim()
                    )));
                }
            }
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
        if !(MOBILE_PULL_CONCURRENCY_MIN..=MOBILE_PULL_CONCURRENCY_MAX).contains(&self.concurrency)
        {
            return Err(AppError::Config(format!(
                "移动设备拉取并发须在 {}–{} 之间，当前为 {}",
                MOBILE_PULL_CONCURRENCY_MIN, MOBILE_PULL_CONCURRENCY_MAX, self.concurrency
            )));
        }
        Ok(())
    }

    /// 钳制后的工作线程数（调用方可在已通过 validate 后使用）。
    pub fn effective_concurrency(&self) -> usize {
        self.concurrency
            .clamp(MOBILE_PULL_CONCURRENCY_MIN, MOBILE_PULL_CONCURRENCY_MAX)
    }

    /// 解析目标 serial 列表。
    pub fn effective_serials(&self) -> Vec<String> {
        self.effective_device_targets()
            .into_iter()
            .map(|t| t.serial)
            .collect()
    }

    /// 解析每台设备的 serial / 来源路径 / 本地保存根目录。
    ///
    /// 保存路径规则：
    /// - 设备指定了 `staging_dir` → 直接用该目录
    /// - 否则 → `staging_dir/<serial>/`（共用一个保存根时按设备建子文件夹）
    pub fn effective_device_targets(&self) -> Vec<ResolvedDeviceTarget> {
        let default_src = self.source_path.trim().to_string();
        let default_staging = self.staging_dir.clone();
        if !self.adb_devices.is_empty() {
            let mut out: Vec<ResolvedDeviceTarget> = Vec::new();
            for device in &self.adb_devices {
                let serial = device.serial.trim();
                if serial.is_empty() {
                    continue;
                }
                if out.iter().any(|t| t.serial == serial) {
                    continue;
                }
                let src = device.resolved_source(&default_src).to_string();
                let staging_root = resolve_device_staging_root(
                    serial,
                    device.staging_override(),
                    &default_staging,
                );
                out.push(ResolvedDeviceTarget {
                    serial: serial.to_string(),
                    source_path: src,
                    staging_root,
                });
            }
            return out;
        }
        self.effective_serials_from_lists()
            .into_iter()
            .map(|serial| {
                let staging_root = resolve_device_staging_root(&serial, None, &default_staging);
                ResolvedDeviceTarget {
                    serial,
                    source_path: default_src.clone(),
                    staging_root,
                }
            })
            .collect()
    }

    fn effective_serials_from_lists(&self) -> Vec<String> {
        if !self.adb_serials.is_empty() {
            return normalize_serial_list(self.adb_serials.iter().cloned());
        }
        if let Some(s) = self.adb_serial.as_deref() {
            return parse_serial_list(s);
        }
        Vec::new()
    }
}

/// 将逗号/空白分隔的 serial 文本解析为列表。
pub fn parse_serial_list(raw: &str) -> Vec<String> {
    normalize_serial_list(
        raw.split([',', ';', ' ', '\t', '\n', '\r'])
            .map(|s| s.to_string()),
    )
}

fn normalize_serial_list(iter: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in iter {
        let t = s.trim();
        if t.is_empty() {
            continue;
        }
        if !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
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

fn default_concurrency() -> usize {
    MOBILE_PULL_CONCURRENCY_DEFAULT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrency_validate_bounds() {
        let mut cfg = MobilePullConfig {
            enabled: true,
            concurrency: 0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
        cfg.concurrency = 9;
        assert!(cfg.validate().is_err());
        cfg.concurrency = 4;
        assert!(cfg.validate().is_ok());
        cfg.concurrency = 1;
        assert!(cfg.validate().is_ok());
        cfg.concurrency = 8;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn concurrency_default_is_four() {
        assert_eq!(MobilePullConfig::default().concurrency, 4);
    }

    #[test]
    fn effective_serials_prefers_list() {
        let cfg = MobilePullConfig {
            adb_serial: Some("old".into()),
            adb_serials: vec!["a".into(), "b".into(), "a".into()],
            ..Default::default()
        };
        assert_eq!(cfg.effective_serials(), vec!["a", "b"]);
    }

    #[test]
    fn effective_serials_falls_back_to_single_with_commas() {
        let cfg = MobilePullConfig {
            adb_serial: Some(" x,y ; z ".into()),
            ..Default::default()
        };
        assert_eq!(cfg.effective_serials(), vec!["x", "y", "z"]);
    }

    #[test]
    fn effective_serials_empty_means_all_ready() {
        assert!(MobilePullConfig::default().effective_serials().is_empty());
    }

    #[test]
    fn parse_serial_list_splits() {
        assert_eq!(parse_serial_list("a,b"), vec!["a", "b"]);
        assert!(parse_serial_list("  , ").is_empty());
    }

    #[test]
    fn effective_device_targets_uses_per_device_paths() {
        let cfg = MobilePullConfig {
            source_path: "/sdcard/DCIM".into(),
            staging_dir: PathBuf::from("/tmp/shared"),
            adb_devices: vec![
                AdbDevicePull::new("a", "/sdcard/DCIM"),
                AdbDevicePull::with_paths(
                    "b",
                    "/sdcard/Pictures",
                    Some(PathBuf::from("/tmp/phone-b")),
                ),
                AdbDevicePull {
                    serial: "c".into(),
                    source_path: None,
                    staging_dir: None,
                },
            ],
            ..Default::default()
        };
        let targets = cfg.effective_device_targets();
        assert_eq!(targets.len(), 3);
        assert_eq!(targets[0].serial, "a");
        assert_eq!(targets[0].source_path, "/sdcard/DCIM");
        assert_eq!(targets[0].staging_root, PathBuf::from("/tmp/shared/a"));
        assert_eq!(targets[1].serial, "b");
        assert_eq!(targets[1].source_path, "/sdcard/Pictures");
        assert_eq!(targets[1].staging_root, PathBuf::from("/tmp/phone-b"));
        assert_eq!(targets[2].serial, "c");
        assert_eq!(targets[2].source_path, "/sdcard/DCIM");
        assert_eq!(targets[2].staging_root, PathBuf::from("/tmp/shared/c"));
        assert_eq!(cfg.effective_serials(), vec!["a", "b", "c"]);
    }

    #[test]
    fn effective_device_targets_falls_back_to_serial_lists() {
        let cfg = MobilePullConfig {
            source_path: "/sdcard/DCIM".into(),
            staging_dir: PathBuf::from("/tmp/shared"),
            adb_serials: vec!["x".into(), "y".into()],
            ..Default::default()
        };
        let targets = cfg.effective_device_targets();
        assert_eq!(
            targets,
            vec![
                ResolvedDeviceTarget {
                    serial: "x".into(),
                    source_path: "/sdcard/DCIM".into(),
                    staging_root: PathBuf::from("/tmp/shared/x"),
                },
                ResolvedDeviceTarget {
                    serial: "y".into(),
                    source_path: "/sdcard/DCIM".into(),
                    staging_root: PathBuf::from("/tmp/shared/y"),
                },
            ]
        );
    }

    #[test]
    fn shared_staging_nests_per_serial() {
        assert_eq!(
            resolve_device_staging_root("ab:cd", None, Path::new("/out")),
            PathBuf::from("/out/ab_cd")
        );
        assert_eq!(
            resolve_device_staging_root(
                "ab:cd",
                Some(&PathBuf::from("/custom/phone")),
                Path::new("/out")
            ),
            PathBuf::from("/custom/phone")
        );
    }

    #[test]
    fn validate_requires_per_device_path_when_default_empty() {
        let cfg = MobilePullConfig {
            enabled: true,
            source_path: String::new(),
            adb_devices: vec![AdbDevicePull::new("a", "/sdcard/DCIM")],
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
        let bad = MobilePullConfig {
            enabled: true,
            source_path: String::new(),
            adb_devices: vec![AdbDevicePull {
                serial: "a".into(),
                source_path: None,
                staging_dir: None,
            }],
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn validate_allows_per_device_staging_when_global_empty() {
        let cfg = MobilePullConfig {
            enabled: true,
            staging_dir: PathBuf::new(),
            adb_devices: vec![AdbDevicePull::with_paths(
                "a",
                "/sdcard/DCIM",
                Some(PathBuf::from("/tmp/a")),
            )],
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }
}
