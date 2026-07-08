//! 画质调整步骤：亮度、对比度、锐化。

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::processing::backends::native_backend::{apply_brightness_contrast, apply_sharpen};
use crate::processing::pipeline::ProcessStep;

/// 亮度/对比度/锐化调整。
pub struct AdjustStep;

impl ProcessStep for AdjustStep {
    fn name(&self) -> &'static str {
        "adjust"
    }

    fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
        if !ctx.adjust.is_active() {
            return Ok(());
        }

        let image = match ctx.image.as_mut() {
            Some(img) => img,
            None => return Ok(()),
        };

        apply_brightness_contrast(image, ctx.adjust.brightness, ctx.adjust.contrast);
        apply_sharpen(image, ctx.adjust.sharpen);
        Ok(())
    }
}
