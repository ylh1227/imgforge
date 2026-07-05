//! 缩略图预生成与缓存。

use std::path::{Path, PathBuf};

use image::GenericImageView;

use crate::processing::image_quality::resize_image;
use crate::core::types::ResizeMode;

use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::{thumbnail_cache_dir, SqliteReviewRepository};

const THUMB_MAX: u32 = 320;

/// 缩略图服务：大图为预览优先加载缩略图，放大时加载原图。
pub struct ThumbnailService;

impl ThumbnailService {
  /// 确保缩略图存在并返回路径。
  pub fn ensure_thumbnail(repo: &SqliteReviewRepository, image_id: i64, source: &Path) -> ReviewResult<PathBuf> {
    if let Ok(item) = repo.get_image(image_id) {
      if let Some(ref thumb) = item.thumbnail_path {
        if thumb.exists() {
          return Ok(thumb.clone());
        }
      }
    }
    let thumb_path = Self::generate(source)?;
    repo.set_thumbnail_path(image_id, &thumb_path)?;
    Ok(thumb_path)
  }

  pub fn generate(source: &Path) -> ReviewResult<PathBuf> {
    let cache_dir = thumbnail_cache_dir()?;
    std::fs::create_dir_all(&cache_dir)?;
    let key = format!("{:x}", md5_path(source));
    let dest = cache_dir.join(format!("{key}.jpg"));
    if dest.exists() {
      return Ok(dest);
    }
    let img = image::open(source).map_err(|source_err| ReviewError::ImageDecode {
      path: source.to_path_buf(),
      source: source_err,
    })?;
    let thumb = resize_thumb(&img);
    thumb.save_with_format(&dest, image::ImageFormat::Jpeg)?;
    Ok(dest)
  }
}

fn resize_thumb(img: &image::DynamicImage) -> image::DynamicImage {
  let (w, h) = img.dimensions();
  if w <= THUMB_MAX && h <= THUMB_MAX {
    return img.clone();
  }
  resize_image(img, Some(THUMB_MAX), Some(THUMB_MAX), ResizeMode::Fit)
    .unwrap_or_else(|_| img.clone())
}

fn md5_path(path: &Path) -> u64 {
  use std::collections::hash_map::DefaultHasher;
  use std::hash::{Hash, Hasher};
  let mut h = DefaultHasher::new();
  path.to_string_lossy().hash(&mut h);
  h.finish()
}
