//! Photoshop 风格画质处理：线性光缩放、USM 锐化、Fill 裁切。

use fast_image_resize as fir;
use fast_image_resize::images::Image as FirImage;
use image::{DynamicImage, Rgba, RgbaImage};

use crate::core::error::{AppError, AppResult};
use crate::core::types::ResizeMode;

/// 对图像应用 USM（Unsharp Mask）锐化，类似 Photoshop「USM / 智能锐化」。
pub fn apply_unsharp_mask(
  image: &mut DynamicImage,
  radius: f32,
  amount: f32,
  threshold: u8,
) {
  if amount <= 0.0 {
    return;
  }
  let radius = radius.clamp(0.3, 5.0);
  let amount = amount.clamp(0.0, 2.0);
  let threshold = threshold.min(255);

  let rgba = image.to_rgba8();
  let blurred = image::imageops::blur(&rgba, radius);
  let mut out = rgba.clone();
  let thr = i16::from(threshold);

  for ((orig, blur), dst) in rgba
    .pixels()
    .zip(blurred.pixels())
    .zip(out.pixels_mut())
  {
    for channel in 0..3 {
      let o = i16::from(orig.0[channel]);
      let b = i16::from(blur.0[channel]);
      let diff = o - b;
      let sharpened = if diff.abs() >= thr {
        (o as f32 + diff as f32 * amount).round() as i32
      } else {
        i32::from(o)
      };
      dst.0[channel] = sharpened.clamp(0, 255) as u8;
    }
    dst.0[3] = orig.0[3];
  }

  *image = DynamicImage::ImageRgba8(out);
}

/// 兼容旧接口：将 amount 映射为 USM 参数。
pub fn apply_sharpen(image: &mut DynamicImage, amount: f32) {
  if amount <= 0.0 {
    return;
  }
  let factor = amount.clamp(0.0, 2.0);
  apply_unsharp_mask(image, 0.8 + factor * 0.5, factor * 0.5, 3);
}

/// 高质量缩放（线性光 Lanczos / Mitchell + 缩小后 USM）。
pub fn resize_image(
  image: &DynamicImage,
  width: Option<u32>,
  height: Option<u32>,
  mode: ResizeMode,
) -> AppResult<DynamicImage> {
  let (src_w, src_h) = (image.width(), image.height());
  let (target_w, target_h) = match (width, height) {
    (Some(w), Some(h)) => (w.max(1), h.max(1)),
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

  if target_w == src_w && target_h == src_h && mode != ResizeMode::Fill {
    return Ok(image.clone());
  }

  let (resize_w, resize_h) = compute_resize_dimensions(src_w, src_h, target_w, target_h, mode);
  if resize_w == src_w && resize_h == src_h && mode != ResizeMode::Fill {
    return Ok(image.clone());
  }

  let scale = (resize_w as f32 / src_w as f32).min(resize_h as f32 / src_h as f32);
  let mut resized = resize_linear_light(image, resize_w, resize_h, scale)?;

  if mode == ResizeMode::Fill && (resize_w != target_w || resize_h != target_h) {
    resized = center_crop(&resized, target_w, target_h)?;
  }

  if scale < 1.0 {
    apply_auto_downscale_sharpen(&mut resized, scale);
  }

  Ok(resized)
}

fn compute_resize_dimensions(
  src_w: u32,
  src_h: u32,
  target_w: u32,
  target_h: u32,
  mode: ResizeMode,
) -> (u32, u32) {
  match mode {
    ResizeMode::Exact => (target_w, target_h),
    ResizeMode::Fill => {
      let ratio_w = target_w as f64 / src_w as f64;
      let ratio_h = target_h as f64 / src_h as f64;
      let ratio = ratio_w.max(ratio_h);
      (
        (src_w as f64 * ratio).round().max(1.0) as u32,
        (src_h as f64 * ratio).round().max(1.0) as u32,
      )
    }
    ResizeMode::Fit => {
      let ratio_w = target_w as f64 / src_w as f64;
      let ratio_h = target_h as f64 / src_h as f64;
      let ratio = ratio_w.min(ratio_h);
      (
        (src_w as f64 * ratio).round().max(1.0) as u32,
        (src_h as f64 * ratio).round().max(1.0) as u32,
      )
    }
  }
}

fn center_crop(image: &DynamicImage, width: u32, height: u32) -> AppResult<DynamicImage> {
  let (src_w, src_h) = (image.width(), image.height());
  if src_w <= width && src_h <= height {
    return Ok(image.clone());
  }
  let x = src_w.saturating_sub(width) / 2;
  let y = src_h.saturating_sub(height) / 2;
  let crop_w = width.min(src_w);
  let crop_h = height.min(src_h);
  Ok(DynamicImage::ImageRgba8(
    image::imageops::crop_imm(image, x, y, crop_w, crop_h).to_image(),
  ))
}

fn resize_linear_light(
  image: &DynamicImage,
  dst_w: u32,
  dst_h: u32,
  scale: f32,
) -> AppResult<DynamicImage> {
  let (src_w, src_h) = (image.width(), image.height());
  let rgba = image.to_rgba8();
  let mut linear = Vec::with_capacity((src_w * src_h * 4) as usize);
  for pixel in rgba.pixels() {
    linear.push(srgb_to_linear(pixel[0]));
    linear.push(srgb_to_linear(pixel[1]));
    linear.push(srgb_to_linear(pixel[2]));
    linear.push(pixel[3] as f32 / 255.0);
  }

  let src_bytes = f32_slice_as_bytes(&linear);
  let src_image = FirImage::from_vec_u8(src_w, src_h, src_bytes, fir::PixelType::F32x4).map_err(
    |e| AppError::Pipeline {
      step: "resize".into(),
      reason: e.to_string(),
    },
  )?;

  let mut dst_image = FirImage::new(dst_w, dst_h, fir::PixelType::F32x4);
  let resize_options = pick_resize_alg(scale);
  let mut resizer = fir::Resizer::new();
  resizer
    .resize(&src_image, &mut dst_image, &resize_options)
    .map_err(|e| AppError::Pipeline {
      step: "resize".into(),
      reason: e.to_string(),
    })?;

  let dst_linear = bytes_as_f32_slice(dst_image.buffer());
  let mut out = RgbaImage::new(dst_w, dst_h);
  for (idx, pixel) in out.pixels_mut().enumerate() {
    let base = idx * 4;
    *pixel = Rgba([
      linear_to_srgb(dst_linear[base]),
      linear_to_srgb(dst_linear[base + 1]),
      linear_to_srgb(dst_linear[base + 2]),
      (dst_linear[base + 3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
    ]);
  }

  Ok(DynamicImage::ImageRgba8(out))
}

fn pick_resize_alg(scale: f32) -> fir::ResizeOptions {
  if scale > 1.0 {
    // 放大：Mitchell（接近 PS「双三次平滑」）
    fir::ResizeOptions::new().resize_alg(fir::ResizeAlg::Convolution(
      fir::FilterType::Mitchell,
    ))
  } else if scale < 0.5 {
    // 强缩小：超采样 + Lanczos3（接近 PS「双三次 sharper / 保留细节」）
    fir::ResizeOptions::new().resize_alg(fir::ResizeAlg::SuperSampling(
      fir::FilterType::Lanczos3,
      2,
    ))
  } else {
    fir::ResizeOptions::new().resize_alg(fir::ResizeAlg::Convolution(
      fir::FilterType::Lanczos3,
    ))
  }
}

fn apply_auto_downscale_sharpen(image: &mut DynamicImage, scale: f32) {
  if scale >= 0.75 {
    return;
  }
  let amount = if scale < 0.25 {
    0.45
  } else if scale < 0.5 {
    0.32
  } else {
    0.22
  };
  let radius = if scale < 0.25 { 1.2 } else if scale < 0.5 { 1.0 } else { 0.8 };
  apply_unsharp_mask(image, radius, amount, 4);
}

#[inline]
fn srgb_to_linear(v: u8) -> f32 {
  let u = v as f32 / 255.0;
  if u <= 0.04045 {
    u / 12.92
  } else {
    ((u + 0.055) / 1.055).powf(2.4)
  }
}

#[inline]
fn linear_to_srgb(v: f32) -> u8 {
  let v = v.clamp(0.0, 1.0);
  let u = if v <= 0.0031308 {
    v * 12.92
  } else {
    1.055 * v.powf(1.0 / 2.4) - 0.055
  };
  (u * 255.0 + 0.5) as u8
}

fn f32_slice_as_bytes(values: &[f32]) -> Vec<u8> {
  let mut bytes = Vec::with_capacity(values.len() * 4);
  for value in values {
    bytes.extend_from_slice(&value.to_ne_bytes());
  }
  bytes
}

fn bytes_as_f32_slice(bytes: &[u8]) -> &[f32] {
  debug_assert_eq!(bytes.len() % 4, 0);
  unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len() / 4) }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::types::ResizeMode;

  #[test]
  fn fill_mode_center_crops_to_target() {
    let mut img = RgbaImage::new(200, 100);
    for (x, y, px) in img.enumerate_pixels_mut() {
      *px = Rgba([x as u8, y as u8, 128, 255]);
    }
    let source = DynamicImage::ImageRgba8(img);
    let out = resize_image(
      &source,
      Some(100),
      Some(100),
      ResizeMode::Fill,
    )
    .unwrap();
    assert_eq!(out.width(), 100);
    assert_eq!(out.height(), 100);
  }

  #[test]
  fn unsharp_mask_increases_local_contrast() {
    let mut img = RgbaImage::new(32, 32);
    for y in 0..32 {
      for x in 0..32 {
        let v = if ((x / 4) + (y / 4)) % 2 == 0 { 40 } else { 200 };
        img.put_pixel(x, y, Rgba([v, v, v, 255]));
      }
    }
    let before = DynamicImage::ImageRgba8(img.clone());
    let edge_before = img.get_pixel(8, 8).0[0];
    let mut sharpened = before;
    apply_unsharp_mask(&mut sharpened, 1.0, 0.8, 0);
    let edge_after = sharpened.to_rgba8().get_pixel(8, 8).0[0];
    assert!(edge_after.abs_diff(edge_before) > 0);
  }
}
