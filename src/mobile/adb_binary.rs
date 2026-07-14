//! ADB 二进制定位。

use std::path::PathBuf;

use crate::core::error::{AppError, AppResult};
use crate::mobile::{AdbBinaryMode, MobilePullConfig};

/// 解析可执行的 ADB 路径。返回 `adb` 时表示使用 PATH。
pub fn resolve_adb_binary(config: &MobilePullConfig) -> AppResult<PathBuf> {
    let mut attempts = Vec::new();

    match config.adb_mode {
        AdbBinaryMode::Custom => {
            return validate_custom(config.adb_path.clone(), &mut attempts);
        }
        AdbBinaryMode::Bundled => {
            return bundled_adb_candidates()
                .into_iter()
                .find(|path| is_executable_file(path))
                .ok_or_else(|| {
                    AppError::Config(format!(
                        "bundled adb not found; checked {}",
                        attempts_from_candidates(bundled_adb_candidates())
                    ))
                });
        }
        AdbBinaryMode::Path => return Ok(PathBuf::from(adb_file_name())),
        AdbBinaryMode::Auto => {}
    }

    if let Some(path) = &config.adb_path {
        attempts.push(path.display().to_string());
        if is_executable_file(path) {
            return Ok(path.clone());
        }
    }

    for path in bundled_adb_candidates() {
        attempts.push(path.display().to_string());
        if is_executable_file(&path) {
            return Ok(path);
        }
    }

    if config.allow_path_fallback {
        return Ok(PathBuf::from(adb_file_name()));
    }

    Err(AppError::Config(format!(
        "adb not found; checked {}",
        attempts.join(", ")
    )))
}

fn validate_custom(path: Option<PathBuf>, attempts: &mut Vec<String>) -> AppResult<PathBuf> {
    let Some(path) = path else {
        return Err(AppError::Config(
            "mobile adb mode 'custom' requires mobile.adb_path".into(),
        ));
    };
    attempts.push(path.display().to_string());
    if is_executable_file(&path) {
        Ok(path)
    } else {
        Err(AppError::Config(format!(
            "custom adb is not executable: {}",
            path.display()
        )))
    }
}

fn bundled_adb_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            if dir.ends_with("Contents/MacOS") {
                if let Some(contents) = dir.parent() {
                    dirs.push(contents.join("Resources"));
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }

    let mut out = Vec::new();
    for dir in dirs {
        out.push(
            dir.join("platform-tools")
                .join(platform_dir())
                .join(adb_file_name()),
        );
        out.push(dir.join("platform-tools").join(adb_file_name()));
        out.push(
            dir.join("resources")
                .join("platform-tools")
                .join(platform_dir())
                .join(adb_file_name()),
        );
    }
    out
}

fn attempts_from_candidates(candidates: Vec<PathBuf>) -> String {
    candidates
        .into_iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_executable_file(path: &PathBuf) -> bool {
    path.is_file()
}

fn adb_file_name() -> &'static str {
    if cfg!(windows) {
        "adb.exe"
    } else {
        "adb"
    }
}

fn platform_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_mode_uses_plain_adb_name() {
        let cfg = MobilePullConfig {
            enabled: true,
            adb_mode: AdbBinaryMode::Path,
            ..MobilePullConfig::default()
        };
        assert_eq!(
            resolve_adb_binary(&cfg).unwrap(),
            PathBuf::from(adb_file_name())
        );
    }
}
