//! 多尺寸缩略图任务扩展（feature: thumbnails）。

use std::path::PathBuf;

use crate::core::types::{ResizeMode, ResizeOptions, ThumbnailSpec};
use crate::scheduler::task::ConversionTask;

/// 将每个输入任务扩展为多个不同尺寸的缩略图任务。
pub fn expand_thumbnail_tasks(
    tasks: Vec<ConversionTask>,
    specs: &[ThumbnailSpec],
) -> Vec<ConversionTask> {
    if specs.is_empty() {
        return tasks;
    }

    let mut expanded = Vec::with_capacity(tasks.len() * specs.len());
    for task in tasks {
        for spec in specs {
            let mut thumb = task.clone();
            thumb.output_path = insert_suffix(&task.output_path, &spec.suffix);
            thumb.resize_override = Some(ResizeOptions {
                width: Some(spec.width),
                height: spec.height,
                mode: ResizeMode::Fit,
            });
            expanded.push(thumb);
        }
    }
    expanded
}

fn insert_suffix(path: &PathBuf, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or(std::path::Path::new(""));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    if ext.is_empty() {
        parent.join(format!("{stem}{suffix}"))
    } else {
        parent.join(format!("{stem}{suffix}.{ext}"))
    }
}
