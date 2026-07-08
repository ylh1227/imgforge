//! 水印叠加步骤（feature: watermark）。

use std::fs;
use std::path::Path;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::imageops;
use image::{DynamicImage, Rgba, RgbaImage};

use crate::core::context::ImageContext;
use crate::core::error::{AppError, AppResult};
use crate::core::types::WatermarkPosition;
use crate::processing::backends::native_backend::resize_image;
use crate::processing::pipeline::ProcessStep;

/// 文字/图片水印叠加。
pub struct WatermarkStep;

impl ProcessStep for WatermarkStep {
    fn name(&self) -> &'static str {
        "watermark"
    }

    fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
        if !ctx.watermark.is_active() {
            return Ok(());
        }

        let base = match ctx.image.take() {
            Some(img) => img,
            None => return Ok(()),
        };

        let mut result = base;
        if let Some(ref path) = ctx.watermark.image_path {
            apply_image_watermark(&mut result, path, &ctx.watermark)?;
        }
        if let Some(ref text) = ctx.watermark.text {
            apply_text_watermark(&mut result, text, &ctx.watermark)?;
        }

        ctx.image = Some(result);
        Ok(())
    }
}

fn apply_image_watermark(
    base: &mut DynamicImage,
    path: &Path,
    options: &crate::core::types::WatermarkOptions,
) -> AppResult<()> {
    let bytes = fs::read(path).map_err(|e| AppError::io(path, e))?;
    let watermark = image::load_from_memory(&bytes).map_err(|e| AppError::DecodeFailed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let (base_w, base_h) = (base.width(), base.height());
    let max_w = (base_w as f32 * 0.25).max(1.0) as u32;
    let max_h = (base_h as f32 * 0.25).max(1.0) as u32;
    let wm = resize_image(
        &watermark,
        Some(max_w),
        Some(max_h),
        crate::core::types::ResizeMode::Fit,
    )?;

    let wm_rgba = apply_opacity(&wm.to_rgba8(), options.opacity);
    let (x, y) = compute_position(
        base_w,
        base_h,
        wm_rgba.width(),
        wm_rgba.height(),
        options.position,
        options.margin,
    );

    let mut canvas = base.to_rgba8();
    imageops::overlay(&mut canvas, &wm_rgba, i64::from(x), i64::from(y));
    *base = DynamicImage::ImageRgba8(canvas);
    Ok(())
}

fn apply_text_watermark(
    base: &mut DynamicImage,
    text: &str,
    options: &crate::core::types::WatermarkOptions,
) -> AppResult<()> {
    let font_path = options.font_path.as_ref().ok_or_else(|| {
        AppError::Config(
            "text watermark requires --watermark-font or watermark.font_path in config".into(),
        )
    })?;

    let font_data = fs::read(font_path).map_err(|e| AppError::io(font_path, e))?;
    let font = FontRef::try_from_slice(&font_data).map_err(|e| AppError::Config(e.to_string()))?;

    let scale = PxScale::from(options.font_size.max(8.0));
    let scaled = font.as_scaled(scale);
    let pad = 8.0f32;

    let text_width: f32 = text
        .chars()
        .map(|ch| scaled.h_advance(font.glyph_id(ch)))
        .sum();
    let wm_w = (text_width + pad * 2.0).ceil().max(1.0) as u32;
    let wm_h = (scaled.height() + pad * 2.0).ceil().max(1.0) as u32;

    let mut wm = RgbaImage::from_pixel(wm_w, wm_h, Rgba([0, 0, 0, (options.opacity * 64.0) as u8]));

    let mut x_cursor = pad;
    let baseline = pad + scaled.ascent();
    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        if let Some(outlined) =
            font.outline_glyph(glyph_id.with_scale_and_position(scale, point(x_cursor, baseline)))
        {
            outlined.draw(|gx, gy, coverage| {
                let px = wm.get_pixel_mut(gx, gy);
                let alpha = (coverage * options.opacity * 255.0) as u8;
                px.0[0] = 255;
                px.0[1] = 255;
                px.0[2] = 255;
                px.0[3] = px.0[3].saturating_add(alpha);
            });
        }
        x_cursor += scaled.h_advance(glyph_id);
    }

    let (base_w, base_h) = (base.width(), base.height());
    let (x, y) = compute_position(base_w, base_h, wm_w, wm_h, options.position, options.margin);
    let mut canvas = base.to_rgba8();
    imageops::overlay(&mut canvas, &wm, i64::from(x), i64::from(y));
    *base = DynamicImage::ImageRgba8(canvas);
    Ok(())
}

fn apply_opacity(image: &RgbaImage, opacity: f32) -> RgbaImage {
    let alpha_scale = opacity.clamp(0.0, 1.0);
    let mut out = image.clone();
    for px in out.pixels_mut() {
        px.0[3] = ((px.0[3] as f32) * alpha_scale) as u8;
    }
    out
}

fn compute_position(
    base_w: u32,
    base_h: u32,
    wm_w: u32,
    wm_h: u32,
    position: WatermarkPosition,
    margin: u32,
) -> (u32, u32) {
    match position {
        WatermarkPosition::TopLeft => (margin, margin),
        WatermarkPosition::TopRight => (base_w.saturating_sub(wm_w + margin), margin),
        WatermarkPosition::BottomLeft => (margin, base_h.saturating_sub(wm_h + margin)),
        WatermarkPosition::BottomRight => (
            base_w.saturating_sub(wm_w + margin),
            base_h.saturating_sub(wm_h + margin),
        ),
        WatermarkPosition::Center => (
            base_w.saturating_sub(wm_w) / 2,
            base_h.saturating_sub(wm_h) / 2,
        ),
    }
}
