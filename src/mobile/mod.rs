//! 移动设备输入准备：本地挂载目录与内置 ADB 拉取。

mod adb;
mod adb_binary;
mod config;
mod fs_pull;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use config::{AdbBinaryMode, MobilePullBackend, MobilePullConfig};

use crate::config::AppConfig;
use crate::core::error::{AppError, AppResult};
use crate::core::types::ImageFormat;
use crate::processing::backends::is_raw_camera_extension;
use crate::ui::progress::ProgressReporter;

/// 移动设备拉取结果。
#[derive(Debug, Clone)]
pub struct MobilePullOutcome {
    pub staging_dir: PathBuf,
    pub files: Vec<PathBuf>,
}

/// 在批量转换前准备输入。未启用移动拉取时原样返回配置。
pub fn prepare_inputs(
    mut config: AppConfig,
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> AppResult<AppConfig> {
    if !config.mobile_pull.enabled {
        return Ok(config);
    }

    if cancelled.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }

    let outcome = pull(&config.mobile_pull, cancelled, progress)?;
    config.input_dir = outcome.staging_dir;
    if !outcome.files.is_empty() {
        config.explicit_inputs = outcome.files;
    }
    Ok(config)
}

fn pull(
    config: &MobilePullConfig,
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> AppResult<MobilePullOutcome> {
    match config.backend {
        MobilePullBackend::Fs => fs_pull::pull(config, cancelled, progress),
        MobilePullBackend::Adb => adb::pull(config, cancelled, progress),
        MobilePullBackend::Auto => {
            if fs_source_available(config) {
                match fs_pull::pull(config, Arc::clone(&cancelled), progress.clone()) {
                    Ok(outcome) => return Ok(outcome),
                    Err(err) => tracing::warn!(error = %err, "mobile fs pull failed; trying adb"),
                }
            }
            adb::pull(config, cancelled, progress)
        }
    }
}

fn fs_source_available(config: &MobilePullConfig) -> bool {
    Path::new(&config.source_path).exists()
}

fn is_supported_media_path(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    is_supported_media_extension(&ext)
}

fn is_supported_media_remote(path: &str) -> bool {
    let ext = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    is_supported_media_extension(&ext)
}

fn is_supported_media_extension(ext: &str) -> bool {
    ImageFormat::from_extension(ext).is_some()
        || is_raw_camera_extension(ext)
        || is_video_media_extension(ext)
}

/// 手机 / 运动相机常见视频后缀（不依赖 video-review feature）。
fn is_video_media_extension(ext: &str) -> bool {
    matches!(
        ext,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "mts" | "m2ts" | "lrv"
    )
}

/// 从手机、运动相机等设备导入图片/视频到本地暂存目录。
///
/// GUI / CLI 共用；调用方需在后台线程执行。
pub fn import_media(
    mut config: MobilePullConfig,
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> AppResult<MobilePullOutcome> {
    config.enabled = true;
    config.validate()?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    pull(&config, cancelled, progress)
}

fn safe_remote_relative(source_root: &str, remote_path: &str) -> AppResult<PathBuf> {
    let rel = remote_path
        .strip_prefix(source_root.trim_end_matches('/'))
        .unwrap_or(remote_path)
        .trim_start_matches('/');
    let mut out = PathBuf::new();
    for part in rel.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(AppError::PathTraversal(PathBuf::from(remote_path)));
        }
        out.push(part);
    }
    if out.as_os_str().is_empty() {
        return Err(AppError::Config(format!(
            "invalid remote media path: {remote_path}"
        )));
    }
    Ok(out)
}

fn ensure_cancelled_not_set(cancelled: &AtomicBool) -> AppResult<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_relative_rejects_parent_dir() {
        let err = safe_remote_relative("/sdcard/DCIM", "/sdcard/DCIM/../secret.jpg").unwrap_err();
        assert!(matches!(err, AppError::PathTraversal(_)));
    }

    #[test]
    fn remote_relative_strips_root() {
        let rel = safe_remote_relative("/sdcard/DCIM", "/sdcard/DCIM/Camera/a.jpg").unwrap();
        assert_eq!(rel, PathBuf::from("Camera/a.jpg"));
    }

    #[test]
    fn media_filter_accepts_photos_and_videos() {
        assert!(is_supported_media_extension("jpg"));
        assert!(is_supported_media_extension("png"));
        assert!(is_supported_media_extension("mp4"));
        assert!(is_supported_media_extension("mov"));
        assert!(is_supported_media_extension("lrv"));
        assert!(!is_supported_media_extension("txt"));
    }
}
