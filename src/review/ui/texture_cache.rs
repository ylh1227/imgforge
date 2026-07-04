//! 图片纹理加载与 egui 缓存（评审/对比视图共用）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Context, TextureHandle};
use image::GenericImageView;

/// 已缓存的图片纹理与原始尺寸。
#[derive(Clone)]
pub struct CachedImage {
  pub texture: TextureHandle,
  pub size: (u32, u32),
  pub source_path: PathBuf,
}

/// 按路径缓存 `TextureHandle`，避免重复解码。
#[derive(Default)]
pub struct ImageTextureCache {
  entries: HashMap<String, CachedImage>,
}

impl ImageTextureCache {
  /// 加载图片（可选缩略图路径优先），命中缓存则直接返回。
  pub fn load(
    &mut self,
    ctx: &Context,
    path: &Path,
    thumb: Option<&Path>,
  ) -> Option<&CachedImage> {
    let key = path.to_string_lossy().to_string();
    if self.entries.contains_key(&key) {
      return self.entries.get(&key);
    }
    let load_path = thumb.filter(|p| p.exists()).unwrap_or(path);
    let img = image::open(load_path).ok()?;
    let size = img.dimensions();
    let rgba = img.to_rgba8();
    let color_image =
      egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    let tex = ctx.load_texture(
      format!("review_tex_{key}"),
      color_image,
      egui::TextureOptions::LINEAR,
    );
    self.entries.insert(
      key.clone(),
      CachedImage {
        texture: tex,
        size,
        source_path: path.to_path_buf(),
      },
    );
    self.entries.get(&key)
  }

  pub fn invalidate(&mut self, path: &Path) {
    self.entries.remove(&path.to_string_lossy().to_string());
  }

  pub fn clear(&mut self) {
    self.entries.clear();
  }
}
