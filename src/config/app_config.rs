//! 核心应用配置结构体。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::types::{
  AdjustOptions, Concurrency, ImageFormat, MetadataPolicy, Quality, ResizeOptions, ThumbnailSpec,
  Transform, WatermarkOptions,
};

/// 单文件转换参数覆盖（评审标记联动带入队列）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConvertOverride {
  pub format: Option<ImageFormat>,
  pub quality: Option<Quality>,
  pub width: Option<u32>,
}

impl ConvertOverride {
  pub fn is_empty(&self) -> bool {
    self.format.is_none() && self.quality.is_none() && self.width.is_none()
  }
}

/// 应用运行时配置，可由 CLI / 环境变量 / TOML 合并而来。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
  pub input_dir: PathBuf,
  pub output_dir: PathBuf,
  pub target_format: ImageFormat,
  pub quality: Quality,
  pub concurrency: Concurrency,
  pub recursive: bool,
  pub overwrite: bool,
  pub preserve_structure: bool,
  pub dry_run: bool,
  pub resize: ResizeOptions,
  pub adjust: AdjustOptions,
  pub metadata_policy: MetadataPolicy,
  pub transform: Transform,
  pub extensions: Vec<String>,
  pub min_size: Option<u64>,
  pub max_size: Option<u64>,
  pub verbose: bool,
  /// 增量处理：仅转换新增/修改的文件（需 incremental feature）。
  pub incremental: bool,
  /// 输出文件名模板（需 rename feature），占位符：{stem} {name} {ext} {dir} {index} {width} {height}。
  pub rename_template: Option<String>,
  /// 水印配置（需 watermark feature）。
  pub watermark: WatermarkOptions,
  /// 多尺寸缩略图规格（需 thumbnails feature）。
  pub thumbnails: Vec<ThumbnailSpec>,
  /// 若非空，仅转换这些文件（评审联动队列），跳过目录扫描。
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub explicit_inputs: Vec<PathBuf>,
  /// 单文件转换参数覆盖（评审标记带入队列），按输入路径匹配。
  #[serde(default, skip_serializing_if = "HashMap::is_empty")]
  pub per_input_params: HashMap<PathBuf, ConvertOverride>,
  /// 导出时叠加评审标注到输出图（需 review feature）。
  #[serde(default)]
  pub burn_review_annotations: bool,
  /// 仅解 Bayer/RAW 马赛克：跳过缩放/锐化/水印等后处理。
  #[serde(default)]
  pub bayer_only: bool,
  /// 目标输出文件大小上限（字节）；启用后对 JPEG/WebP 等做质量二分拟合。
  #[serde(default)]
  pub target_max_bytes: Option<u64>,
}

impl Default for AppConfig {
  fn default() -> Self {
    Self {
      input_dir: PathBuf::from("."),
      output_dir: PathBuf::from("./output"),
      target_format: ImageFormat::WebP,
      quality: Quality::DEFAULT,
      concurrency: Concurrency::default_parallel(),
      recursive: true,
      overwrite: false,
      preserve_structure: true,
      dry_run: false,
      resize: ResizeOptions {
        width: None,
        height: None,
        mode: crate::core::types::ResizeMode::Fit,
      },
      adjust: AdjustOptions::default(),
      metadata_policy: MetadataPolicy::Preserve,
      transform: Transform::None,
      extensions: Vec::new(),
      min_size: None,
      max_size: None,
      verbose: false,
      incremental: false,
      rename_template: None,
      watermark: WatermarkOptions::default(),
      thumbnails: Vec::new(),
      explicit_inputs: Vec::new(),
      per_input_params: HashMap::new(),
      burn_review_annotations: false,
      bayer_only: false,
      target_max_bytes: None,
    }
  }
}

impl AppConfig {
  /// 加载后统一校验配置合法性。
  pub fn validate(&self) -> crate::core::error::AppResult<()> {
    if !self.input_dir.exists() {
      return Err(crate::core::error::AppError::Config(format!(
        "input directory does not exist: {}",
        self.input_dir.display()
      )));
    }
    if self.concurrency.value() < 1 {
      return Err(crate::core::error::AppError::InvalidConcurrency(
        self.concurrency.value(),
      ));
    }
    Ok(())
  }
}
