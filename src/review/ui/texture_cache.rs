//! 图片纹理加载与 egui 缓存（三级异步 + LRU）。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Context, TextureHandle};

use crate::review::domain::coords::ViewportTransform;
use crate::review::service::{
    is_non_filesystem_path, AsyncImageLoader, DecodedImage, ImageLoadTier, LoadOutcome,
};

/// 每帧最多上传的纹理数量，避免窗口缩放/列表展开时主线程卡顿。
const MAX_TEXTURE_UPLOADS_PER_FRAME: usize = 2;

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
    /// 路径级失败信息（停止无限重试）。
    failures: HashMap<String, String>,
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
            failures: HashMap::new(),
        }
    }

    pub fn loader(&self) -> &AsyncImageLoader {
        &self.loader
    }

    /// 请求加载（异步）；若已缓存同层或更高层则跳过。
    pub fn request(&mut self, path: &Path, thumb: Option<&Path>, tier: ImageLoadTier) {
        if is_non_filesystem_path(path) {
            return;
        }
        let key = path.to_string_lossy().to_string();
        if self.failures.contains_key(&key) {
            return;
        }
        if self.current_tier.get(&key).is_some_and(|t| *t >= tier) {
            return;
        }
        let _ = self.loader.request(path, thumb, tier);
    }

    /// 轮询后台解码结果并上传纹理。
    pub fn poll(&mut self, ctx: &Context) -> bool {
        let mut uploaded = false;
        for _ in 0..MAX_TEXTURE_UPLOADS_PER_FRAME {
            let Some(outcome) = self.loader.try_recv_one() else {
                break;
            };
            match outcome {
                LoadOutcome::Ok(img) => {
                    self.insert_decoded(ctx, img);
                    uploaded = true;
                }
                LoadOutcome::Err(fail) => {
                    let path_key = fail.path.to_string_lossy().to_string();
                    self.failures.insert(path_key, fail.error);
                }
            }
        }
        uploaded
    }

    pub fn load_error(&self, path: &Path) -> Option<&str> {
        let key = path.to_string_lossy().to_string();
        self.failures.get(&key).map(String::as_str)
    }

    pub fn clear_error(&mut self, path: &Path) {
        let key = path.to_string_lossy().to_string();
        self.failures.remove(&key);
    }

    fn insert_decoded(&mut self, ctx: &Context, img: DecodedImage) {
        let path_key = img.path.to_string_lossy().to_string();
        self.failures.remove(&path_key);
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
                source_path: img.path,
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
        self.failures.remove(&key);
        self.order.retain(|k| k != &key);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_tier.clear();
        self.failures.clear();
        self.order.clear();
    }

    pub fn prefetch_neighbors(
        &self,
        paths: &[PathBuf],
        center: usize,
        radius: usize,
        thumb_paths: &[Option<PathBuf>],
    ) {
        self.loader
            .prefetch_neighbors(paths, center, radius, thumb_paths);
    }

    /// 批量预取缩略图（多图对比进入时调用）。
    pub fn prefetch_thumbs(&mut self, items: &[(PathBuf, Option<PathBuf>)]) {
        for (path, thumb) in items {
            self.request(path, thumb.as_deref(), ImageLoadTier::Thumb);
        }
    }

    pub fn ensure_capacity(&mut self, min_entries: usize) {
        self.max_entries = self.max_entries.max(min_entries.max(4));
    }

    /// 按屏幕显示比例请求合适分辨率（避免 fit 视图误触发原图全量解码）。
    pub fn maybe_upgrade(
        &mut self,
        path: &Path,
        thumb: Option<&Path>,
        viewport: ViewportTransform,
        image_size: (u32, u32),
        max_tier: ImageLoadTier,
    ) {
        let scale = effective_display_scale(viewport, image_size);
        let tier = if scale >= 0.98 {
            ImageLoadTier::Full
        } else if scale >= 0.35 {
            ImageLoadTier::Preview
        } else {
            ImageLoadTier::Thumb
        };
        self.request(path, thumb, tier.min(max_tier));
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

/// 屏幕像素 / 原图像素（取宽高较大比值）。
fn effective_display_scale(viewport: ViewportTransform, image_size: (u32, u32)) -> f32 {
    let iw = image_size.0.max(1) as f32;
    let ih = image_size.1.max(1) as f32;
    let displayed_w = viewport.image_rect.size.x * viewport.zoom;
    let displayed_h = viewport.image_rect.size.y * viewport.zoom;
    (displayed_w / iw).max(displayed_h / ih)
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
