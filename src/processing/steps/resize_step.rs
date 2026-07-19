//! 旋转变换步骤（不做缩放：缩放会损失清晰度）。

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::core::types::Transform;
use crate::processing::pipeline::ProcessStep;

/// 仅旋转/翻转；忽略宽高缩放配置。
pub struct ResizeStep;

impl ProcessStep for ResizeStep {
    fn name(&self) -> &'static str {
        "resize"
    }

    fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
        let image = match ctx.image.take() {
            Some(img) => img,
            None => return Ok(()),
        };

        if ctx.resize.is_active() {
            tracing::warn!(
                path = %ctx.source_path.display(),
                "跳过缩放（保留清晰度）；如需改尺寸请在外部处理"
            );
        }

        // 90° 旋转/翻转不改变像素信息量，允许保留。
        let result = apply_transform(image, ctx.transform);
        ctx.image = Some(result);
        Ok(())
    }
}

fn apply_transform(image: image::DynamicImage, transform: Transform) -> image::DynamicImage {
    match transform {
        Transform::None => image,
        Transform::Rotate90 => image.rotate90(),
        Transform::Rotate180 => image.rotate180(),
        Transform::Rotate270 => image.rotate270(),
        Transform::FlipHorizontal => image.fliph(),
        Transform::FlipVertical => image.flipv(),
    }
}
