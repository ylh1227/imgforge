//! 纯 Rust 原生图像编解码后端，基于 `image` crate。

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ImageFormat as ImageCrateFormat};

use crate::core::context::ImageContext;
use crate::core::error::{AppError, AppResult};
use crate::core::types::ImageFormat;
use crate::processing::backends::webp_codec;
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

    if ctx.bayer_only || crate::processing::backends::is_raw_camera_path(&ctx.source_path) {
      let image = crate::processing::backends::decode_bayer_only(bytes, &ctx.source_path)?;
      ctx.source_format = None;
      ctx.image = Some(image);
      return Ok(());
    }

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

    let mut image = image::load_from_memory(bytes).map_err(|e| AppError::DecodeFailed {
      path: ctx.source_path.clone(),
      reason: e.to_string(),
    })?;
    apply_exif_orientation(bytes, &mut image);

    ctx.image = Some(image);
    Ok(())
  }

  fn encode(&self, ctx: &mut ImageContext) -> AppResult<()> {
    let image = ctx.image.as_ref().ok_or_else(|| AppError::Pipeline {
      step: "encode".into(),
      reason: "no image data available".into(),
    })?;

    let buffer = encode_dynamic_image(image, ctx.target_format, ctx.quality)?;
    ctx.encoded_bytes = Some(buffer);
    ctx.output_size = ctx.encoded_bytes.as_ref().map(|b| b.len() as u64).unwrap_or(0);
    Ok(())
  }
}

/// 将内存图像按目标格式与质量编码（供流水线与标注烧录复用）。
pub fn encode_dynamic_image(
  image: &DynamicImage,
  target_format: ImageFormat,
  quality: crate::core::types::Quality,
) -> AppResult<Vec<u8>> {
  let q = quality.value();

  match target_format {
    ImageFormat::Jpeg => {
      let mut buffer = Vec::new();
      let rgb = image.to_rgb8();
      let mut encoder = JpegEncoder::new_with_quality(&mut buffer, q);
      encoder
        .encode(
          rgb.as_raw(),
          rgb.width(),
          rgb.height(),
          image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| encode_err(target_format, e))?;
      Ok(buffer)
    }
    ImageFormat::Png => {
      let mut buffer = Vec::new();
      let compression = if quality.is_lossless() {
        CompressionType::Best
      } else {
        CompressionType::Default
      };
      let encoder = PngEncoder::new_with_quality(&mut buffer, compression, FilterType::Adaptive);
      DynamicImage::ImageRgba8(image.to_rgba8())
        .write_with_encoder(encoder)
        .map_err(|e| encode_err(target_format, e))?;
      Ok(buffer)
    }
    ImageFormat::WebP => {
      if quality.is_lossless() {
        webp_codec::encode_webp_lossless(image)
      } else {
        webp_codec::encode_webp_lossy(image, q)
      }
    }
    ImageFormat::Bmp => {
      let mut buffer = Vec::new();
      image
        .write_to(&mut Cursor::new(&mut buffer), ImageCrateFormat::Bmp)
        .map_err(|e| encode_err(target_format, e))?;
      Ok(buffer)
    }
    ImageFormat::Tiff => {
      let mut buffer = Vec::new();
      image
        .write_to(&mut Cursor::new(&mut buffer), ImageCrateFormat::Tiff)
        .map_err(|e| encode_err(target_format, e))?;
      Ok(buffer)
    }
    ImageFormat::Gif => {
      let mut buffer = Vec::new();
      image
        .write_to(&mut Cursor::new(&mut buffer), ImageCrateFormat::Gif)
        .map_err(|e| encode_err(target_format, e))?;
      Ok(buffer)
    }
    #[cfg(feature = "avif")]
    ImageFormat::Avif => crate::processing::backends::avif_codec::encode_avif(image, q),
    #[cfg(feature = "jpegxl")]
    ImageFormat::JpegXl => crate::processing::backends::jxl_codec::encode_jpegxl(image, q),
  }
}

#[cfg(test)]
mod encode_tests {
  use super::*;
  use crate::core::types::Quality;

  #[test]
  fn webp_lossy_respects_quality_setting() {
    let mut rgba = image::RgbaImage::new(128, 128);
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
      *pixel = image::Rgba([(x * 3) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255]);
    }
    let img = DynamicImage::ImageRgba8(rgba);
    let small = encode_dynamic_image(&img, ImageFormat::WebP, Quality::new(30).unwrap()).unwrap();
    let large = encode_dynamic_image(&img, ImageFormat::WebP, Quality::new(95).unwrap()).unwrap();
    assert!(small.starts_with(b"RIFF"));
    assert!(large.starts_with(b"RIFF"));
    assert!(large.len() > small.len());
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

fn apply_exif_orientation(bytes: &[u8], image: &mut DynamicImage) {
  use image::metadata::Orientation;

  let Some(exif) = extract_jpeg_exif_segment(bytes) else {
    return;
  };
  if let Some(orientation) = Orientation::from_exif_chunk(&exif) {
    image.apply_orientation(orientation);
  }
}

fn extract_jpeg_exif_segment(data: &[u8]) -> Option<Vec<u8>> {
  if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
    return None;
  }
  let mut offset = 2;
  while offset + 4 < data.len() {
    if data[offset] != 0xFF {
      break;
    }
    let marker = data[offset + 1];
    if marker == 0xE1 {
      let len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
      if offset + 2 + len <= data.len() {
        let segment = &data[offset + 4..offset + 2 + len];
        if segment.starts_with(b"Exif\0\0") {
          return Some(segment.to_vec());
        }
      }
    }
    if marker == 0xDA {
      break;
    }
    let seg_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
    offset += 2 + seg_len;
  }
  None
}

/// 对图像应用锐化（USM）。
pub fn apply_sharpen(image: &mut DynamicImage, amount: f32) {
  crate::processing::image_quality::apply_sharpen(image, amount);
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

/// 高质量缩放（委托 image_quality 模块）。
pub fn resize_image(
  image: &DynamicImage,
  width: Option<u32>,
  height: Option<u32>,
  mode: crate::core::types::ResizeMode,
) -> AppResult<DynamicImage> {
  crate::processing::image_quality::resize_image(image, width, height, mode)
}
