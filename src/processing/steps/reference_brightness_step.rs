//! 参考图外观匹配步骤。
//!
//! 目标：RAW 解马赛克后贴近同名 JPG（Vitrine：矩阵+曲线+LUT）；仅解 Bayer 整段跳过。

use tracing::{debug, info};

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::processing::backends::is_raw_camera_path;
use crate::processing::brightness_match::apply_brightness_match_gain_fallback;
use crate::processing::camera_match;
use crate::processing::pipeline::ProcessStep;

/// Resize 之后、手动 Adjust 之前执行参考匹配。
pub struct ReferenceBrightnessStep;

impl ProcessStep for ReferenceBrightnessStep {
    fn name(&self) -> &'static str {
        "reference_brightness"
    }

    fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
        if ctx.bayer_only || !ctx.brightness_match.is_active() {
            return Ok(());
        }

        let Some(cache) = ctx.brightness_match_cache.as_ref() else {
            return Ok(());
        };
        let Some(reference) = cache.resolve_reference(&ctx.source_path, &ctx.brightness_match)?
        else {
            if is_raw_camera_path(&ctx.source_path) {
                debug!(
                    path = %ctx.source_path.display(),
                    "RAW 无同名 JPG，跳过亮度匹配"
                );
            } else {
                debug!(
                    path = %ctx.source_path.display(),
                    "无可用参考图，跳过亮度匹配"
                );
            }
            return Ok(());
        };
        let Some(image) = ctx.image.as_mut() else {
            return Ok(());
        };

        if camera_match::try_apply_camera_match(image, reference.as_ref()) {
            info!(
                path = %ctx.source_path.display(),
                "camera-match 已施加（矩阵+曲线+LUT）"
            );
        } else {
            debug!(
                path = %ctx.source_path.display(),
                "camera-match 拟合失败，回退亮度增益"
            );
            apply_brightness_match_gain_fallback(image, reference.as_ref(), &ctx.brightness_match);
        }
        Ok(())
    }
}
