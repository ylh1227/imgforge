//! 并行目录扫描与多维度过滤。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jwalk::WalkDir;

use crate::core::error::{AppError, AppResult};
use crate::core::types::ImageFormat;
use crate::io::paths::{self, ensure_safe_relative};
use crate::scheduler::task::ConversionTask;

/// 扫描过滤器。
#[derive(Debug, Clone, Default)]
pub struct ScanFilter {
  pub extensions: Vec<String>,
  pub min_size: Option<u64>,
  pub max_size: Option<u64>,
  pub modified_after: Option<SystemTime>,
  pub modified_before: Option<SystemTime>,
}

/// 扫描选项。
#[derive(Debug, Clone)]
pub struct ScanOptions {
  pub input_dir: PathBuf,
  pub output_dir: PathBuf,
  pub target_format: ImageFormat,
  pub recursive: bool,
  pub preserve_structure: bool,
  pub overwrite: bool,
  pub filter: ScanFilter,
  pub rename_template: Option<String>,
}

/// 并行扫描输入目录，生成转换任务列表。
pub fn scan_inputs(options: &ScanOptions) -> AppResult<Vec<ConversionTask>> {
  if !options.input_dir.exists() {
    return Err(AppError::Config(format!(
      "input directory does not exist: {}",
      options.input_dir.display()
    )));
  }

  let walker = if options.recursive {
    WalkDir::new(&options.input_dir)
  } else {
    WalkDir::new(&options.input_dir).max_depth(1)
  };

  let input_canon = paths::canonicalize(&options.input_dir);

  let mut tasks = Vec::new();
  let mut index = 0usize;

  for entry in walker.into_iter().filter_map(|e| e.ok()) {
    let path = entry.path();
    if !path.is_file() {
      continue;
    }

    let ext = path
      .extension()
      .and_then(|e| e.to_str())
      .unwrap_or("")
      .to_ascii_lowercase();

    if ImageFormat::from_extension(&ext).is_none() {
      continue;
    }

    if !options.filter.extensions.is_empty()
      && !options.filter.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext))
    {
      continue;
    }

    let metadata = match entry.metadata() {
      Ok(m) => m,
      Err(_) => continue,
    };
    let size = metadata.len();

    if let Some(min) = options.filter.min_size {
      if size < min {
        continue;
      }
    }
    if let Some(max) = options.filter.max_size {
      if size > max {
        continue;
      }
    }

    if let Ok(mtime) = metadata.modified() {
      if let Some(after) = options.filter.modified_after {
        if mtime < after {
          continue;
        }
      }
      if let Some(before) = options.filter.modified_before {
        if mtime > before {
          continue;
        }
      }
    }

    let source_canon = paths::canonicalize(&path);

    let output_path = resolve_output_path(
      &input_canon,
      &source_canon,
      &options.output_dir,
      options.target_format,
      options.preserve_structure,
      options.rename_template.as_deref(),
      index,
    )?;

    index += 1;

    if !options.overwrite && output_path.exists() {
      continue;
    }

    tasks.push(ConversionTask::new(source_canon, output_path, size));
  }

  if tasks.is_empty() {
    return Err(AppError::Config(format!(
      "no image files found in '{}'. Check the folder path and supported formats (jpg, png, webp, …)",
      options.input_dir.display()
    )));
  }

  Ok(tasks)
}

/// 从显式文件列表构建转换任务（评审「通过」队列联动）。
pub fn tasks_from_paths(
  paths: &[PathBuf],
  output_dir: &Path,
  target_format: ImageFormat,
  overwrite: bool,
) -> AppResult<Vec<ConversionTask>> {
  let output_root = paths::canonicalize(output_dir);
  let mut tasks = Vec::new();
  for path in paths {
    if !path.is_file() {
      continue;
    }
    let ext = path
      .extension()
      .and_then(|e| e.to_str())
      .unwrap_or("")
      .to_ascii_lowercase();
    if ImageFormat::from_extension(&ext).is_none() {
      continue;
    }
    let size = std::fs::metadata(path)
      .map_err(|e| AppError::io(path, e))?
      .len();
    let source = paths::canonicalize(path);
    let stem = path
      .file_stem()
      .map(|s| s.to_string_lossy().to_string())
      .unwrap_or_else(|| "output".to_string());
    let output = output_root.join(format!("{stem}.{}", target_format.extension()));
    ensure_safe_relative(&output)?;
    if !overwrite && output.exists() {
      continue;
    }
    tasks.push(ConversionTask::new(source, output, size));
  }
  if tasks.is_empty() {
    return Err(AppError::Config(
      "explicit input list contains no convertible images".into(),
    ));
  }
  Ok(tasks)
}

fn resolve_output_path(
  input_root: &Path,
  source: &Path,
  output_root: &Path,
  target_format: ImageFormat,
  preserve_structure: bool,
  rename_template: Option<&str>,
  index: usize,
) -> AppResult<PathBuf> {
  let relative = paths::relative_path(input_root, source);

  let stem = relative
    .file_stem()
    .map(|s| s.to_string_lossy().to_string())
    .unwrap_or_else(|| "output".to_string());

  let parent = if preserve_structure {
    relative.parent().unwrap_or(Path::new(""))
  } else {
    Path::new("")
  };

  let file_name = if let Some(template) = rename_template {
    #[cfg(feature = "rename")]
    {
      let ctx = crate::io::rename::context_from_path(input_root, source, index, None, None);
      let renamed = crate::io::rename::apply_rename_template(template, &ctx)?;
      if renamed.contains('.') {
        renamed
      } else {
        format!("{renamed}.{}", target_format.extension())
      }
    }
    #[cfg(not(feature = "rename"))]
    {
      let _ = (template, index);
      return Err(AppError::Config(
        "rename template requires --features rename".into(),
      ));
    }
  } else {
    format!("{stem}.{}", target_format.extension())
  };

  let output = output_root.join(parent).join(file_name);

  ensure_safe_relative(&output)?;

  Ok(output)
}
