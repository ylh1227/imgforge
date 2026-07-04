//! WebP 有损编码（libwebp，quality 1–100 生效）。

use image::codecs::webp::WebPEncoder;
use image::DynamicImage;

use crate::core::error::{AppError, AppResult};

/// 将 RGBA 图像编码为有损 WebP。
pub fn encode_webp_lossy(image: &DynamicImage, quality: u8) -> AppResult<Vec<u8>> {
  let rgba = image.to_rgba8();
  let (width, height) = rgba.dimensions();
  let encoded = webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(f32::from(quality));
  Ok(encoded.to_vec())
}

pub fn encode_webp_lossless(image: &DynamicImage) -> AppResult<Vec<u8>> {
  let mut buffer = Vec::new();
  let encoder = WebPEncoder::new_lossless(&mut buffer);
  DynamicImage::ImageRgba8(image.to_rgba8())
    .write_with_encoder(encoder)
    .map_err(|e| AppError::EncodeFailed {
      format: "webp".into(),
      reason: e.to_string(),
    })?;
  Ok(buffer)
}
