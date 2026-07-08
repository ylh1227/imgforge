//! 批量转换任务编排（CLI 与 GUI 共用）。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::core::error::AppResult;
use crate::io::scanner::{ScanFilter, ScanOptions};
use crate::scheduler::Executor;
use crate::ui::progress::ProgressReporter;
use crate::ui::report::ProcessReport;

/// 扫描输入目录并执行批量转换。
pub async fn run_batch(
    config: AppConfig,
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> AppResult<ProcessReport> {
    let scan_options = ScanOptions {
        input_dir: config.input_dir.clone(),
        output_dir: config.output_dir.clone(),
        target_format: config.target_format,
        recursive: config.recursive,
        preserve_structure: config.preserve_structure,
        overwrite: config.overwrite,
        filter: ScanFilter {
            extensions: config.extensions.clone(),
            min_size: config.min_size,
            max_size: config.max_size,
            modified_after: None,
            modified_before: None,
        },
        rename_template: config.rename_template.clone(),
        bayer_only: config.bayer_only,
    };

    let mut tasks = if !config.explicit_inputs.is_empty() {
        crate::io::scanner::tasks_from_paths(
            &config.explicit_inputs,
            &config.output_dir,
            config.target_format,
            config.overwrite,
            config.bayer_only,
        )?
    } else {
        crate::io::scanner::scan_inputs(&scan_options)?
    };

    #[cfg(feature = "thumbnails")]
    if !config.thumbnails.is_empty() {
        tasks = crate::io::thumbnails::expand_thumbnail_tasks(tasks, &config.thumbnails);
    }

    if !config.per_input_params.is_empty() {
        apply_per_input_overrides(&mut tasks, &config);
    }

    let executor = Executor::new(config, cancelled);
    let result = executor.run(tasks, progress).await?;
    Ok(result.report)
}

/// 转换前扫描摘要（不执行编码）。
pub fn preview_batch(config: &AppConfig) -> AppResult<crate::io::batch_preview::BatchPreview> {
    crate::io::batch_preview::preview_batch(config)
}

/// 将评审标记的单图转换参数应用到对应任务（格式/质量/宽度覆盖）。
fn apply_per_input_overrides(
    tasks: &mut [crate::scheduler::task::ConversionTask],
    config: &AppConfig,
) {
    use crate::core::types::ResizeMode;
    use crate::io::paths;

    let by_canon: std::collections::HashMap<std::path::PathBuf, &crate::config::ConvertOverride> =
        config
            .per_input_params
            .iter()
            .map(|(k, v)| (paths::canonicalize(k), v))
            .collect();

    for task in tasks.iter_mut() {
        let Some(ov) = by_canon.get(&task.input_path).copied() else {
            continue;
        };
        if ov.is_empty() {
            continue;
        }
        if let Some(fmt) = ov.format {
            task.format_override = Some(fmt);
            // 输出扩展名跟随目标格式
            task.output_path.set_extension(fmt.extension());
        }
        if let Some(q) = ov.quality {
            task.quality_override = Some(q);
        }
        if let Some(w) = ov.width {
            let mut resize = task.resize_override.unwrap_or(config.resize);
            resize.width = Some(w);
            if resize.mode == ResizeMode::default() {
                resize.mode = ResizeMode::Fit;
            }
            task.resize_override = Some(resize);
        }
    }
}
