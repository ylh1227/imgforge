//! 从已挂载为本地目录的移动设备拉取媒体。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use jwalk::WalkDir;

use crate::core::error::{AppError, AppResult};
use crate::mobile::{
    ensure_cancelled_not_set, is_supported_media_path, run_parallel_jobs, MobilePullConfig,
    MobilePullOutcome,
};
use crate::ui::progress::ProgressReporter;

pub fn pull(
    config: &MobilePullConfig,
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> AppResult<MobilePullOutcome> {
    let source_root = Path::new(&config.source_path);
    if !source_root.is_dir() {
        return Err(AppError::Config(format!(
            "mobile source directory does not exist: {}",
            source_root.display()
        )));
    }

    std::fs::create_dir_all(&config.staging_dir)
        .map_err(|e| AppError::io(&config.staging_dir, e))?;

    let mut sources = Vec::new();
    for entry in WalkDir::new(source_root).into_iter().filter_map(|e| e.ok()) {
        ensure_cancelled_not_set(&cancelled)?;
        let path = entry.path();
        if entry.file_type().is_file() && is_supported_media_path(&path) {
            sources.push(path);
        }
    }

    if let Some(progress) = &progress {
        progress.set_total(sources.len());
        progress.set_current_label("正在从移动设备复制文件");
    }

    struct Job {
        source: PathBuf,
        target: PathBuf,
        need_copy: bool,
        label: String,
    }

    let mut jobs = Vec::with_capacity(sources.len());
    for source in sources {
        ensure_cancelled_not_set(&cancelled)?;
        let relative = if config.preserve_structure {
            source
                .strip_prefix(source_root)
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    source
                        .file_name()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("media"))
                })
        } else {
            source
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("media"))
        };
        if relative.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        }) {
            return Err(AppError::PathTraversal(relative));
        }
        let target = config.staging_dir.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let label = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("media")
            .to_string();
        let need_copy = !target.exists();
        jobs.push(Job {
            source,
            target,
            need_copy,
            label,
        });
    }

    let concurrency = config.effective_concurrency();
    let progress = progress.clone();
    let files = run_parallel_jobs(jobs, concurrency, &cancelled, |job| {
        if job.need_copy {
            std::fs::copy(&job.source, &job.target).map_err(|e| AppError::io(&job.target, e))?;
        }
        if let Some(progress) = &progress {
            progress.set_current_label(&job.label);
            progress.inc(None);
        }
        Ok(job.target.clone())
    })?;

    Ok(MobilePullOutcome {
        staging_dir: config.staging_dir.clone(),
        files,
    })
}
