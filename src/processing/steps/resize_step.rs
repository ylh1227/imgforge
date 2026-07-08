//! 缩放与旋转变换步骤。

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::core::types::Transform;
use crate::processing::backends::native_backend::resize_image;
use crate::processing::pipeline::ProcessStep;

/// 等比缩放、裁剪与旋转变换。
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

        let mut result = if ctx.resize.is_active() {
            resize_image(&image, ctx.resize.width, ctx.resize.height, ctx.resize.mode)?
        } else {
            image
        };

        result = apply_transform(result, ctx.transform);
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
