//! 单文件图像转换任务定义。

use std::path::PathBuf;

use crate::core::types::{ImageFormat, ResizeOptions};

/// 描述一次单文件格式转换任务。
#[derive(Debug, Clone)]
pub struct ConversionTask {
  pub input_path: PathBuf,
  pub output_path: PathBuf,
  pub input_size: u64,
  pub source_format: Option<ImageFormat>,
  /// 任务级缩放覆盖（缩略图等场景）。
  pub resize_override: Option<ResizeOptions>,
}

impl ConversionTask {
  pub fn new(input_path: PathBuf, output_path: PathBuf, input_size: u64) -> Self {
    let source_format = input_path
      .extension()
      .and_then(|e| e.to_str())
      .and_then(ImageFormat::from_extension);
    Self {
      input_path,
      output_path,
      input_size,
      source_format,
      resize_override: None,
    }
  }
}
