//! 图像解码步骤。

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::processing::backends::default_backend;
use crate::processing::pipeline::ProcessStep;

/// 将原始字节解码为内存图像。
pub struct DecodeStep;

impl ProcessStep for DecodeStep {
  fn name(&self) -> &'static str {
    "decode"
  }

  fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
    let backend = default_backend();
    backend.decode(ctx)
  }
}
