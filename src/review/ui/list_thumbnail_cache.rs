//! 侧边栏列表缩略图：磁盘 WebP 缓存 + 后台生成 + 异步解码为 egui 纹理。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Context, TextureHandle};

use crate::review::service::{
    cache_key, AsyncImageLoader, AsyncThumbnailGenerator, ImageLoadTier, LoadOutcome,
    ThumbnailService,
};

const TEXTURE_PREFIX: &str = "review_list_thumb_";
const MAX_LIST_TEXTURE_UPLOADS_PER_FRAME: usize = 2;

/// 列表缩略图纹理缓存（按 image_id 索引）。
pub struct ListThumbnailCache {
    textures: HashMap<i64, TextureHandle>,
    inflight: HashSet<i64>,
    decode_id_by_key: HashMap<String, i64>,
    generator: AsyncThumbnailGenerator,
    loader: AsyncImageLoader,
}

impl Default for ListThumbnailCache {
    fn default() -> Self {
        Self {
            textures: HashMap::new(),
            inflight: HashSet::new(),
            decode_id_by_key: HashMap::new(),
            generator: AsyncThumbnailGenerator::new(),
            loader: AsyncImageLoader::new(1),
        }
    }
}

impl ListThumbnailCache {
    pub fn get(&self, image_id: i64) -> Option<&TextureHandle> {
        self.textures.get(&image_id)
    }

    /// 请求加载列表缩略图（缓存命中则异步解码，否则后台生成 WebP）。
    pub fn request(&mut self, image_id: i64, source: &Path) {
        if self.textures.contains_key(&image_id) || self.inflight.contains(&image_id) {
            return;
        }
        self.inflight.insert(image_id);
        if let Some(cached) = ThumbnailService::valid_cache_path(source) {
            self.queue_decode(image_id, &cached);
        } else {
            self.generator.request(image_id, source.to_path_buf());
        }
    }

    fn queue_decode(&mut self, image_id: i64, path: &Path) {
        let key = cache_key(path, ImageLoadTier::Thumb);
        self.decode_id_by_key.insert(key.clone(), image_id);
        let _ = self.loader.request(path, None, ImageLoadTier::Thumb);
    }

    /// 轮询后台任务；若纹理有更新返回 true（应 request_repaint）。
    pub fn poll(&mut self, ctx: &Context) -> bool {
        let mut dirty = false;
        for (id, path) in self.generator.poll() {
            self.queue_decode(id, &path);
            dirty = true;
        }
        let mut uploaded = 0;
        while uploaded < MAX_LIST_TEXTURE_UPLOADS_PER_FRAME {
            let Some(outcome) = self.loader.try_recv_one() else {
                break;
            };
            match outcome {
                LoadOutcome::Ok(img) => {
                    if let Some(id) = self.decode_id_by_key.remove(&img.key) {
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            [img.width as usize, img.height as usize],
                            &img.rgba,
                        );
                        let tex = ctx.load_texture(
                            format!("{TEXTURE_PREFIX}{id}"),
                            color,
                            egui::TextureOptions::LINEAR,
                        );
                        self.textures.insert(id, tex);
                        self.inflight.remove(&id);
                        dirty = true;
                        uploaded += 1;
                    }
                }
                LoadOutcome::Err(fail) => {
                    if let Some(id) = self.decode_id_by_key.remove(&fail.key) {
                        self.inflight.remove(&id);
                        dirty = true;
                    }
                }
            }
        }
        dirty
    }

    pub fn clear(&mut self) {
        self.textures.clear();
        self.inflight.clear();
        self.decode_id_by_key.clear();
    }
}
