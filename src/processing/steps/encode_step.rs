//! 图像编码步骤。

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::processing::backends::default_backend;
use crate::processing::pipeline::ProcessStep;

/// 将内存图像编码为目标格式字节。
pub struct EncodeStep;

impl ProcessStep for EncodeStep {
    fn name(&self) -> &'static str {
        "encode"
    }

    fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
        let backend = default_backend();
        backend.encode(ctx)
    }
}
