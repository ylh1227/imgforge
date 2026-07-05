//! 图片元数据（纯数据结构 + 轻量读取）。

use std::path::Path;

use image::GenericImageView;
use serde::{Deserialize, Serialize};

use crate::review::error::{ReviewError, ReviewResult};

/// 评审属性面板展示的图片元数据。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageMetadata {
  pub width: Option<u32>,
  pub height: Option<u32>,
  pub bit_depth: Option<u8>,
  pub color_space: Option<String>,
  pub file_size: Option<u64>,
  pub exif_summary: Option<String>,
}

impl ImageMetadata {
  pub fn resolution_label(&self) -> String {
    match (self.width, self.height) {
      (Some(w), Some(h)) => format!("{w} × {h}"),
      _ => "—".into(),
    }
  }

  pub fn file_size_label(&self) -> String {
    self
      .file_size
      .map(format_bytes)
      .unwrap_or_else(|| "—".into())
  }
}

pub fn format_bytes(bytes: u64) -> String {
  const KB: f64 = 1024.0;
  const MB: f64 = KB * 1024.0;
  const GB: f64 = MB * 1024.0;
  let b = bytes as f64;
  if b >= GB {
    format!("{:.2} GB", b / GB)
  } else if b >= MB {
    format!("{:.2} MB", b / MB)
  } else if b >= KB {
    format!("{:.1} KB", b / KB)
  } else {
    format!("{bytes} B")
  }
}

/// 从磁盘读取基础元数据（不阻塞 UI 时应在线程池中调用）。
pub fn read_image_metadata(path: &Path) -> ReviewResult<ImageMetadata> {
  let file_size = std::fs::metadata(path).ok().map(|m| m.len());
  let img = image::open(path).map_err(|source| ReviewError::ImageDecode {
    path: path.to_path_buf(),
    source,
  })?;
  let (width, height) = img.dimensions();
  let color = img.color();
  let bit_depth = Some(color.bytes_per_pixel() as u8 * 8);
  let color_space = Some(format!("{color:?}"));
  Ok(ImageMetadata {
    width: Some(width),
    height: Some(height),
    bit_depth,
    color_space,
    file_size,
    exif_summary: read_exif_summary(path),
  })
}

fn read_exif_summary(_path: &Path) -> Option<String> {
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn format_bytes_scales() {
    assert!(format_bytes(512).contains("B"));
    assert!(format_bytes(2048).contains("KB"));
  }
}
