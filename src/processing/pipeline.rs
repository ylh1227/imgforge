//! 流水线抽象：可插拔处理步骤与执行器。

use crate::config::app_config::AppConfig;
use crate::core::context::ImageContext;
use crate::core::error::{AppError, AppResult};

/// 流水线处理步骤 trait，所有步骤职责单一、可组合。
pub trait ProcessStep: Send + Sync {
  fn name(&self) -> &'static str;
  fn execute(&self, ctx: &mut ImageContext) -> AppResult<()>;
}

/// 可动态组装的图像处理流水线。
pub struct ProcessingPipeline {
  steps: Vec<Box<dyn ProcessStep>>,
}

impl ProcessingPipeline {
  pub fn new() -> Self {
    Self { steps: Vec::new() }
  }

  /// 链式添加处理步骤。
  pub fn add_step(mut self, step: impl ProcessStep + 'static) -> Self {
    self.steps.push(Box::new(step));
    self
  }

  /// 按顺序执行所有步骤。
  pub fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
    for step in &self.steps {
      step.execute(ctx).map_err(|e| match e {
        AppError::Pipeline { .. } => e,
        other => AppError::Pipeline {
          step: step.name().to_string(),
          reason: other.to_string(),
        },
      })?;
    }
    Ok(())
  }

  pub fn step_count(&self) -> usize {
    self.steps.len()
  }
}

impl Default for ProcessingPipeline {
  fn default() -> Self {
    Self::new()
  }
}

/// 根据配置构建处理流水线。
pub fn build_pipeline(config: &AppConfig) -> ProcessingPipeline {
  use crate::processing::steps::{
    adjust_step::AdjustStep, decode_step::DecodeStep, encode_step::EncodeStep,
    metadata_step::MetadataStep, resize_step::ResizeStep,
  };

  let mut pipeline = ProcessingPipeline::new()
    .add_step(DecodeStep)
    .add_step(MetadataStep::read());

  if !config.bayer_only {
    pipeline = pipeline.add_step(ResizeStep).add_step(AdjustStep);

    #[cfg(feature = "watermark")]
    if config.watermark.is_active() {
      use crate::processing::steps::watermark_step::WatermarkStep;
      pipeline = pipeline.add_step(WatermarkStep);
    }
  }

  pipeline
    .add_step(MetadataStep::write())
    .add_step(EncodeStep)
}

/// 兼容旧接口。
pub fn build_default_pipeline() -> ProcessingPipeline {
  build_pipeline(&AppConfig::default())
}
