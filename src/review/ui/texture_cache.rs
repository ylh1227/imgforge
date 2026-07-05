//! 图片纹理加载与 egui 缓存（三级异步 + LRU）。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Context, TextureHandle};
use image::GenericImageView;

use crate::review::service::{AsyncImageLoader, DecodedImage, ImageLoadTier};

/// 已缓存的图片纹理与原始尺寸。
#[derive(Clone)]
pub struct CachedImage {
  pub texture: TextureHandle,
  pub size: (u32, u32),
  pub source_path: PathBuf,
  pub tier: ImageLoadTier,
}

/// 按路径 + 层级缓存纹理，支持异步解码与 LRU 淘汰。
pub struct ImageTextureCache {
  entries: HashMap<String, CachedImage>,
  order: VecDeque<String>,
  max_entries: usize,
  loader: AsyncImageLoader,
  current_tier: HashMap<String, ImageLoadTier>,
}

impl Default for ImageTextureCache {
  fn default() -> Self {
    Self::new(24)
  }
}

impl ImageTextureCache {
  pub fn new(max_entries: usize) -> Self {
    Self {
      entries: HashMap::new(),
      order: VecDeque::new(),
      max_entries: max_entries.max(4),
      loader: AsyncImageLoader::new(2),
      current_tier: HashMap::new(),
    }
  }

  pub fn loader(&self) -> &AsyncImageLoader {
    &self.loader
  }

  /// 请求加载（异步）；若已缓存同层或更高层则跳过。
  pub fn request(
    &mut self,
    path: &Path,
    thumb: Option<&Path>,
    tier: ImageLoadTier,
  ) {
    let key = path.to_string_lossy().to_string();
    if self
      .current_tier
      .get(&key)
      .is_some_and(|t| *t >= tier)
    {
      return;
    }
    let _ = self.loader.request(path, thumb, tier);
  }

  /// 轮询后台解码结果并上传纹理。
  pub fn poll(&mut self, ctx: &Context) {
    let decoded = self.loader.poll();
    for img in decoded {
      self.insert_decoded(ctx, img);
    }
  }

  fn insert_decoded(&mut self, ctx: &Context, img: DecodedImage) {
    let path_key = img
      .key
      .split(':')
      .next()
      .unwrap_or(&img.key)
      .to_string();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
      [img.width as usize, img.height as usize],
      &img.rgba,
    );
    let tex = ctx.load_texture(
      format!("review_tex_{}", img.key),
      color_image,
      egui::TextureOptions::LINEAR,
    );
    self.touch(&path_key);
    self.entries.insert(
      path_key.clone(),
      CachedImage {
        texture: tex,
        size: (img.width, img.height),
        source_path: PathBuf::from(&path_key),
        tier: img.tier,
      },
    );
    self.current_tier.insert(path_key, img.tier);
    self.evict_if_needed();
  }

  /// 同步回退（仅测试或紧急路径）。
  pub fn load_sync(
    &mut self,
    ctx: &Context,
    path: &Path,
    thumb: Option<&Path>,
  ) -> Option<&CachedImage> {
    self.request(path, thumb, ImageLoadTier::Thumb);
    self.poll(ctx);
    self.get(path)
  }

  pub fn get(&self, path: &Path) -> Option<&CachedImage> {
    let key = path.to_string_lossy().to_string();
    self.entries.get(&key)
  }

  pub fn invalidate(&mut self, path: &Path) {
    let key = path.to_string_lossy().to_string();
    self.entries.remove(&key);
    self.current_tier.remove(&key);
    self.order.retain(|k| k != &key);
  }

  pub fn clear(&mut self) {
    self.entries.clear();
    self.current_tier.clear();
    self.order.clear();
  }

  pub fn prefetch_neighbors(
    &self,
    paths: &[PathBuf],
    center: usize,
    radius: usize,
    thumb_paths: &[Option<PathBuf>],
  ) {
    self
      .loader
      .prefetch_neighbors(paths, center, radius, thumb_paths);
  }

  /// 缩放超过阈值时请求更高分辨率。
  pub fn maybe_upgrade(&mut self, path: &Path, thumb: Option<&Path>, zoom: f32) {
    let tier = if zoom >= 1.0 {
      ImageLoadTier::Full
    } else if zoom >= 0.5 {
      ImageLoadTier::Preview
    } else {
      ImageLoadTier::Thumb
    };
    self.request(path, thumb, tier);
  }

  fn touch(&mut self, key: &str) {
    self.order.retain(|k| k != key);
    self.order.push_back(key.to_string());
  }

  fn evict_if_needed(&mut self) {
    while self.entries.len() > self.max_entries {
      if let Some(old) = self.order.pop_front() {
        self.entries.remove(&old);
        self.current_tier.remove(&old);
      } else {
        break;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cache_key_tier_ordering() {
    assert!(ImageLoadTier::Full > ImageLoadTier::Preview);
    assert!(ImageLoadTier::Preview > ImageLoadTier::Thumb);
  }
}
