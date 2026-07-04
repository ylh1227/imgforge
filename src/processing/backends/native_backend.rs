//! 纯 Rust 原生图像编解码后端，基于 `image` crate。

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageFormat as ImageCrateFormat};

use crate::core::context::ImageContext;
use crate::core::error::{AppError, AppResult};
use crate::core::types::ImageFormat;
use crate::processing::backends::ImageBackend;

/// 默认纯 Rust 后端实现。
pub struct NativeBackend;

impl NativeBackend {
  pub fn new() -> Self {
    Self
  }
}

impl Default for NativeBackend {
  fn default() -> Self {
    Self::new()
  }
}

impl ImageBackend for NativeBackend {
  fn name(&self) -> &'static str {
    "native"
  }

  fn supported_formats(&self) -> &[ImageFormat] {
    // 核心格式；AVIF/JXL 在启用对应 feature 时由编解码器额外支持。
    &[
      ImageFormat::Jpeg,
      ImageFormat::Png,
      ImageFormat::WebP,
      ImageFormat::Bmp,
      ImageFormat::Tiff,
      ImageFormat::Gif,
    ]
  }

  fn decode(&self, ctx: &mut ImageContext) -> AppResult<()> {
    let bytes = ctx
      .raw_bytes
      .as_ref()
      .ok_or_else(|| AppError::Pipeline {
        step: "decode".into(),
        reason: "no raw bytes available".into(),
      })?;

    // AVIF / JPEG XL 优先走专用解码器
    #[cfg(feature = "avif")]
    if is_avif(bytes, &ctx.source_path) {
      let image = crate::processing::backends::avif_codec::decode_avif(bytes)?;
      ctx.source_format = Some(ImageFormat::Avif);
      ctx.image = Some(image);
      return Ok(());
    }

    #[cfg(feature = "jpegxl")]
    if is_jpegxl(bytes, &ctx.source_path) {
      let image = crate::processing::backends::jxl_codec::decode_jpegxl(bytes)?;
      ctx.source_format = Some(ImageFormat::JpegXl);
      ctx.image = Some(image);
      return Ok(());
    }

    let format = image::guess_format(bytes).map_err(|e| AppError::DecodeFailed {
      path: ctx.source_path.clone(),
      reason: e.to_string(),
    })?;

    let source_format = map_crate_format(format);
    ctx.source_format = Some(source_format);

    let image = image::load_from_memory(bytes).map_err(|e| AppError::DecodeFailed {
      path: ctx.source_path.clone(),
      reason: e.to_string(),
    })?;

    ctx.image = Some(image);
    Ok(())
  }

  fn encode(&self, ctx: &mut ImageContext) -> AppResult<()> {
    let image = ctx.image.as_ref().ok_or_else(|| AppError::Pipeline {
      step: "encode".into(),
      reason: "no image data available".into(),
    })?;

    let mut buffer = Vec::new();
    let quality = ctx.quality.value();

    match ctx.target_format {
      ImageFormat::Jpeg => {
        let mut encoder = JpegEncoder::new_with_quality(&mut buffer, quality);
        encoder
          .encode_image(image)
          .map_err(|e| encode_err(ctx.target_format, e))?;
      }
      ImageFormat::Png => {
        let compression = if ctx.quality.is_lossless() {
          CompressionType::Best
        } else {
          CompressionType::Default
        };
        let encoder = PngEncoder::new_with_quality(&mut buffer, compression, FilterType::Adaptive);
        DynamicImage::ImageRgba8(image.to_rgba8())
          .write_with_encoder(encoder)
          .map_err(|e| encode_err(ctx.target_format, e))?;
      }
      ImageFormat::WebP => {
        if ctx.quality.is_lossless() {
          let encoder = WebPEncoder::new_lossless(&mut buffer);
          DynamicImage::ImageRgba8(image.to_rgba8())
            .write_with_encoder(encoder)
            .map_err(|e| encode_err(ctx.target_format, e))?;
        } else {
          image
            .write_to(&mut Cursor::new(&mut buffer), ImageCrateFormat::WebP)
            .map_err(|e| encode_err(ctx.target_format, e))?;
        }
      }
      ImageFormat::Bmp => {
        image
          .write_to(&mut Cursor::new(&mut buffer), ImageCrateFormat::Bmp)
          .map_err(|e| encode_err(ctx.target_format, e))?;
      }
      ImageFormat::Tiff => {
        image
          .write_to(&mut Cursor::new(&mut buffer), ImageCrateFormat::Tiff)
          .map_err(|e| encode_err(ctx.target_format, e))?;
      }
      ImageFormat::Gif => {
        image
          .write_to(&mut Cursor::new(&mut buffer), ImageCrateFormat::Gif)
          .map_err(|e| encode_err(ctx.target_format, e))?;
      }
      #[cfg(feature = "avif")]
      ImageFormat::Avif => {
        let buffer = crate::processing::backends::avif_codec::encode_avif(image, quality)?;
        ctx.encoded_bytes = Some(buffer);
        ctx.output_size = ctx.encoded_bytes.as_ref().map(|b| b.len() as u64).unwrap_or(0);
        return Ok(());
      }
      #[cfg(feature = "jpegxl")]
      ImageFormat::JpegXl => {
        let buffer = crate::processing::backends::jxl_codec::encode_jpegxl(image, quality)?;
        ctx.encoded_bytes = Some(buffer);
        ctx.output_size = ctx.encoded_bytes.as_ref().map(|b| b.len() as u64).unwrap_or(0);
        return Ok(());
      }
    }

    ctx.encoded_bytes = Some(buffer);
    ctx.output_size = ctx.encoded_bytes.as_ref().map(|b| b.len() as u64).unwrap_or(0);
    Ok(())
  }
}

#[cfg(feature = "avif")]
fn is_avif(bytes: &[u8], path: &std::path::Path) -> bool {
  path
    .extension()
    .and_then(|e| e.to_str())
    .is_some_and(|e| e.eq_ignore_ascii_case("avif"))
    || bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && bytes.get(8..12) == Some(b"avif")
}

#[cfg(feature = "jpegxl")]
fn is_jpegxl(bytes: &[u8], path: &std::path::Path) -> bool {
  path
    .extension()
    .and_then(|e| e.to_str())
    .is_some_and(|e| e.eq_ignore_ascii_case("jxl"))
    || bytes.starts_with(&[0xFF, 0x0A])
    || bytes.starts_with(b"\0\0\0\x0CJXL ")
}

fn map_crate_format(format: ImageCrateFormat) -> ImageFormat {
  match format {
    ImageCrateFormat::Jpeg => ImageFormat::Jpeg,
    ImageCrateFormat::Png => ImageFormat::Png,
    ImageCrateFormat::WebP => ImageFormat::WebP,
    ImageCrateFormat::Bmp => ImageFormat::Bmp,
    ImageCrateFormat::Tiff => ImageFormat::Tiff,
    ImageCrateFormat::Gif => ImageFormat::Gif,
    _ => ImageFormat::Png,
  }
}

fn encode_err(format: ImageFormat, err: image::ImageError) -> AppError {
  AppError::EncodeFailed {
    format: format.to_string(),
    reason: err.to_string(),
  }
}

/// 对图像应用锐化滤镜（简单卷积核）。
pub fn apply_sharpen(image: &mut DynamicImage, amount: f32) {
  if amount <= 0.0 {
    return;
  }
  let factor = amount.clamp(0.0, 2.0);
  *image = image.filter3x3(&[
    0.0,
    -factor,
    0.0,
    -factor,
    1.0 + 4.0 * factor,
    -factor,
    0.0,
    -factor,
    0.0,
  ]);
}

/// 亮度/对比度调整。
pub fn apply_brightness_contrast(image: &mut DynamicImage, brightness: f32, contrast: f32) {
  if brightness != 0.0 {
    *image = image.brighten((brightness * 50.0) as i32);
  }
  if contrast != 0.0 {
    let c = (1.0 + contrast).clamp(0.1, 3.0);
    *image = image.adjust_contrast(c);
  }
}

/// 高质量缩放。
pub fn resize_image(
  image: &DynamicImage,
  width: Option<u32>,
  height: Option<u32>,
  mode: crate::core::types::ResizeMode,
) -> AppResult<DynamicImage> {
  use crate::core::types::ResizeMode;
  use fast_image_resize as fir;
  use fast_image_resize::images::Image as FirImage;

  let (src_w, src_h) = (image.width(), image.height());
  let (target_w, target_h) = match (width, height) {
    (Some(w), Some(h)) => (w, h),
    (Some(w), None) => {
      let ratio = w as f64 / src_w as f64;
      (w, (src_h as f64 * ratio).round().max(1.0) as u32)
    }
    (None, Some(h)) => {
      let ratio = h as f64 / src_h as f64;
      ((src_w as f64 * ratio).round().max(1.0) as u32, h)
    }
    (None, None) => return Ok(image.clone()),
  };

  if target_w == src_w && target_h == src_h {
    return Ok(image.clone());
  }

  let rgba = image.to_rgba8();
  let src_image = FirImage::from_vec_u8(
    src_w,
    src_h,
    rgba.into_raw(),
    fir::PixelType::U8x4,
  )
  .map_err(|e| AppError::Pipeline {
    step: "resize".into(),
    reason: e.to_string(),
  })?;

  let (dst_w, dst_h) = match mode {
    ResizeMode::Exact | ResizeMode::Fill => (target_w, target_h),
    ResizeMode::Fit => {
      let ratio_w = target_w as f64 / src_w as f64;
      let ratio_h = target_h as f64 / src_h as f64;
      let ratio = ratio_w.min(ratio_h);
      (
        (src_w as f64 * ratio).round().max(1.0) as u32,
        (src_h as f64 * ratio).round().max(1.0) as u32,
      )
    }
  };

  let mut dst_image = FirImage::new(dst_w, dst_h, fir::PixelType::U8x4);
  let mut resizer = fir::Resizer::new();
  resizer
    .resize(&src_image, &mut dst_image, None)
    .map_err(|e| AppError::Pipeline {
      step: "resize".into(),
      reason: e.to_string(),
    })?;

  let mut result =
    image::RgbaImage::from_raw(dst_w, dst_h, dst_image.into_vec()).ok_or_else(|| AppError::Pipeline {
      step: "resize".into(),
      reason: "failed to reconstruct image buffer".into(),
    })?;

  if mode == ResizeMode::Fill && (dst_w != target_w || dst_h != target_h) {
    let x = (target_w.saturating_sub(dst_w)) / 2;
    let y = (target_h.saturating_sub(dst_h)) / 2;
    let mut canvas = image::RgbaImage::new(target_w, target_h);
    image::imageops::overlay(&mut canvas, &result, i64::from(x), i64::from(y));
    result = canvas;
  }

  Ok(DynamicImage::ImageRgba8(result))
}
