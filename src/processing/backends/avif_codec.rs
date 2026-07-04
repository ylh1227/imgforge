//! AVIF 编解码（feature: avif / avif-decode）。

use image::DynamicImage;
use ravif::{Encoder, Img};
use rgb::FromSlice;

use crate::core::error::{AppError, AppResult};

/// 使用 ravif 将 RGBA 图像编码为 AVIF（纯 Rust，feature: avif）。
pub fn encode_avif(image: &DynamicImage, quality: u8) -> AppResult<Vec<u8>> {
  let rgba = image.to_rgba8();
  let (width, height) = rgba.dimensions();
  let pixels = rgba.as_raw().as_rgba();
  let img = Img::new(pixels, width as usize, height as usize);
  let encoder = Encoder::new()
    .with_quality(quality as f32)
    .with_speed(avif_speed_for_quality(quality));

  let encoded = encoder
    .encode_rgba(img)
    .map_err(|e| AppError::EncodeFailed {
      format: "avif".into(),
      reason: e.to_string(),
    })?;

  Ok(encoded.avif_file)
}

/// 质量越高编码越慢、细节保留越好（ravif speed 1=最慢最好，10=最快）。
fn avif_speed_for_quality(quality: u8) -> u8 {
  match quality {
    95..=100 => 2,
    85..=94 => 4,
    70..=84 => 6,
    _ => 8,
  }
}

/// 解码 AVIF 输入（feature: avif-decode，需 cmake/libaom）。
#[cfg(feature = "avif-decode")]
pub fn decode_avif(bytes: &[u8]) -> AppResult<DynamicImage> {
  use avif_decode::{Decoder, Image as AvifImage};

  let decoder = Decoder::from_avif(bytes).map_err(|e| AppError::DecodeFailed {
    path: "avif".into(),
    reason: e.to_string(),
  })?;

  match decoder.to_image().map_err(|e| AppError::DecodeFailed {
    path: "avif".into(),
    reason: e.to_string(),
  })? {
    AvifImage::Rgba8(img) => {
      let (w, h) = (img.width() as u32, img.height() as u32);
      let buffer: Vec<u8> = img
        .pixels()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect();
      RgbaImage::from_raw(w, h, buffer)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| AppError::DecodeFailed {
          path: "avif".into(),
          reason: "failed to build RGBA image".into(),
        })
    }
    AvifImage::Rgb8(img) => {
      let (w, h) = (img.width() as u32, img.height() as u32);
      let mut rgba = Vec::with_capacity(img.width() * img.height() * 4);
      for p in img.pixels() {
        rgba.extend_from_slice(&[p.r, p.g, p.b, 255]);
      }
      RgbaImage::from_raw(w, h, rgba)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| AppError::DecodeFailed {
          path: "avif".into(),
          reason: "failed to build RGBA image from RGB".into(),
        })
    }
    other => Err(AppError::DecodeFailed {
      path: "avif".into(),
      reason: format!("unsupported AVIF pixel format: {other:?}"),
    }),
  }
}

/// 未启用 avif-decode 时的占位解码。
#[cfg(all(feature = "avif", not(feature = "avif-decode")))]
pub fn decode_avif(_bytes: &[u8]) -> AppResult<DynamicImage> {
  Err(AppError::DecodeFailed {
    path: "avif".into(),
    reason: "AVIF decoding requires --features avif-decode (needs cmake/libaom)".into(),
  })
}
