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

  let executor = Executor::new(config, cancelled);
  let result = executor.run(tasks, progress).await?;
  Ok(result.report)
}
