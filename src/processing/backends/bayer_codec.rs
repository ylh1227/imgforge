//! 相机 RAW Bayer 仅解马赛克（不做白平衡/色彩矩阵等 RAW 开发流程）。

use std::io::Cursor;
use std::path::Path;

use demosaic::{demosaic_interleaved, Algorithm, CfaPattern, Channel};
use image::{DynamicImage, Rgba, RgbaImage};
use rawloader::{Orientation, RawImage, RawImageData, CFA};

use crate::core::error::{AppError, AppResult};

/// 常见相机 RAW 扩展名（与 rawloader 支持范围大致一致）。
pub fn is_raw_camera_extension(ext: &str) -> bool {
  matches!(
    ext.to_ascii_lowercase().as_str(),
    "3fr"
      | "arw"
      | "cr2"
      | "cr3"
      | "crw"
      | "dcr"
      | "dng"
      | "erf"
      | "iiq"
      | "kdc"
      | "mef"
      | "mos"
      | "mrw"
      | "nef"
      | "nrw"
      | "orf"
      | "pef"
      | "raf"
      | "raw"
      | "rw2"
      | "rwl"
      | "sr2"
      | "srf"
  )
}

pub fn is_raw_camera_path(path: &Path) -> bool {
  path
    .extension()
    .and_then(|e| e.to_str())
    .is_some_and(is_raw_camera_extension)
}

/// 从 RAW 字节仅做 Bayer/X-Trans 解马赛克，输出 sRGB RGB8。
pub fn decode_bayer_only(bytes: &[u8], path: &Path) -> AppResult<DynamicImage> {
  let mut cursor = Cursor::new(bytes);
  let raw = rawloader::decode(&mut cursor).map_err(|e| AppError::DecodeFailed {
    path: path.to_path_buf(),
    reason: e.to_string(),
  })?;
  demosaic_raw_image(raw, path)
}

fn demosaic_raw_image(raw: RawImage, path: &Path) -> AppResult<DynamicImage> {
  let [top, right, bottom, left] = raw.crops;
  let width = raw.width.saturating_sub(left + right);
  let height = raw.height.saturating_sub(top + bottom);
  if width == 0 || height == 0 {
    return Err(AppError::DecodeFailed {
      path: path.to_path_buf(),
      reason: "invalid RAW crop dimensions".into(),
    });
  }

  let cfa = raw.cropped_cfa();
  let (pattern, algorithm) = map_cfa(&cfa).ok_or_else(|| AppError::DecodeFailed {
    path: path.to_path_buf(),
    reason: format!("unsupported CFA pattern: {}", cfa.to_string()),
  })?;

  let mut input = Vec::with_capacity(width * height);
  match raw.data {
    RawImageData::Integer(data) => {
      for y in 0..height {
        for x in 0..width {
          let sx = x + left;
          let sy = y + top;
          let idx = sy * raw.width + sx;
          let value = *data.get(idx).ok_or_else(|| AppError::DecodeFailed {
            path: path.to_path_buf(),
            reason: "RAW integer buffer out of bounds".into(),
          })?;
          let ch = cfa.color_at(sy, sx).min(3);
          input.push(normalize_u16(value, raw.blacklevels[ch], raw.whitelevels[ch]));
        }
      }
    }
    RawImageData::Float(data) => {
      for y in 0..height {
        for x in 0..width {
          let sx = x + left;
          let sy = y + top;
          let idx = sy * raw.width + sx;
          let value = *data.get(idx).ok_or_else(|| AppError::DecodeFailed {
            path: path.to_path_buf(),
            reason: "RAW float buffer out of bounds".into(),
          })?;
          input.push(value.clamp(0.0, 1.0));
        }
      }
    }
  }

  let mut rgb = vec![0.0f32; width * height * 3];
  demosaic_interleaved(&input, width, height, &pattern, algorithm, &mut rgb).map_err(|e| {
    AppError::DecodeFailed {
      path: path.to_path_buf(),
      reason: e.to_string(),
    }
  })?;

  let mut rgba = RgbaImage::new(width as u32, height as u32);
  for y in 0..height {
    for x in 0..width {
      let idx = (y * width + x) * 3;
      rgba.put_pixel(
        x as u32,
        y as u32,
        Rgba([
          linear_to_srgb8(rgb[idx]),
          linear_to_srgb8(rgb[idx + 1]),
          linear_to_srgb8(rgb[idx + 2]),
          255,
        ]),
      );
    }
  }

  let mut image = DynamicImage::ImageRgba8(rgba);
  apply_orientation(&mut image, raw.orientation);
  Ok(image)
}

fn map_cfa(cfa: &CFA) -> Option<(CfaPattern, Algorithm)> {
  if cfa.width == 2 && cfa.height == 2 {
    let pattern = match (
      cfa.color_at(0, 0),
      cfa.color_at(0, 1),
      cfa.color_at(1, 0),
      cfa.color_at(1, 1),
    ) {
      (0, 1, 1, 2) => CfaPattern::bayer_rggb(),
      (2, 1, 1, 0) => CfaPattern::bayer_bggr(),
      (1, 0, 2, 1) => CfaPattern::bayer_grbg(),
      (1, 2, 0, 1) => CfaPattern::bayer_gbrg(),
      _ => return None,
    };
    return Some((pattern, Algorithm::Ahd));
  }

  if cfa.width == 4 && cfa.height == 4 {
    let pattern = match (
      cfa.color_at(0, 0),
      cfa.color_at(0, 1),
      cfa.color_at(1, 0),
      cfa.color_at(1, 1),
    ) {
      (0, 1, 1, 2) => CfaPattern::quad_bayer_rggb(),
      (2, 1, 1, 0) => CfaPattern::quad_bayer_bggr(),
      (1, 0, 2, 1) => CfaPattern::quad_bayer_grbg(),
      (1, 2, 0, 1) => CfaPattern::quad_bayer_gbrg(),
      _ => return None,
    };
    return Some((pattern, Algorithm::QuadPpg));
  }

  if cfa.width == 6 && cfa.height == 6 {
    let mut pattern = [Channel::Green; 36];
    for row in 0..6 {
      for col in 0..6 {
        pattern[row * 6 + col] = match cfa.color_at(row, col) {
          0 => Channel::Red,
          1 => Channel::Green,
          2 => Channel::Blue,
          _ => Channel::Green,
        };
      }
    }
    return Some((CfaPattern::xtrans(pattern), Algorithm::Markesteijn1));
  }

  None
}

fn normalize_u16(value: u16, black: u16, white: u16) -> f32 {
  if white <= black {
    return 0.0;
  }
  ((value as f32 - black as f32) / (white as f32 - black as f32)).clamp(0.0, 1.0)
}

#[inline]
fn linear_to_srgb8(v: f32) -> u8 {
  let v = v.clamp(0.0, 1.0);
  let u = if v <= 0.0031308 {
    v * 12.92
  } else {
    1.055 * v.powf(1.0 / 2.4) - 0.055
  };
  (u * 255.0 + 0.5) as u8
}

fn apply_orientation(image: &mut DynamicImage, orientation: Orientation) {
  *image = match orientation {
    Orientation::Normal | Orientation::Unknown => image.clone(),
    Orientation::HorizontalFlip => image.fliph(),
    Orientation::Rotate180 => image.rotate180(),
    Orientation::VerticalFlip => image.flipv(),
    Orientation::Transpose => image.rotate90().fliph(),
    Orientation::Rotate90 => image.rotate90(),
    Orientation::Transverse => image.rotate270().fliph(),
    Orientation::Rotate270 => image.rotate270(),
  };
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn recognizes_common_raw_extensions() {
    assert!(is_raw_camera_extension("dng"));
    assert!(is_raw_camera_extension("CR2"));
    assert!(!is_raw_camera_extension("jpg"));
  }
}
