//! 画质调整步骤：仅亮度/对比度；不做锐化（锐化会改清晰度）。

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::processing::backends::native_backend::apply_brightness_contrast;
use crate::processing::pipeline::ProcessStep;

/// 亮度/对比度调整；锐化一律跳过。
pub struct AdjustStep;

impl ProcessStep for AdjustStep {
    fn name(&self) -> &'static str {
        "adjust"
    }

    fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
        if ctx.adjust.brightness == 0.0 && ctx.adjust.contrast == 0.0 {
            if ctx.adjust.sharpen > 0.0 {
                tracing::warn!(
                    path = %ctx.source_path.display(),
                    "跳过锐化（保留清晰度）"
                );
            }
            return Ok(());
        }

        let image = match ctx.image.as_mut() {
            Some(img) => img,
            None => return Ok(()),
        };

        apply_brightness_contrast(image, ctx.adjust.brightness, ctx.adjust.contrast);
        if ctx.adjust.sharpen > 0.0 {
            tracing::warn!(
                path = %ctx.source_path.display(),
                "跳过锐化（保留清晰度）"
            );
        }
        Ok(())
    }
}
