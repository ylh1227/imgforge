//! 缩略图预生成与本地持久化缓存（WebP 有损，mtime 失效校验）。

use std::path::{Path, PathBuf};

use image::GenericImageView;

use crate::core::types::{ImageFormat, Quality, ResizeMode};
use crate::processing::backends::native_backend::encode_dynamic_image;
use crate::processing::image_quality::resize_image;

use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::{thumbnail_cache_dir, SqliteReviewRepository};

/// 缩略图统一宽度（对齐 XnView MP 列表缩略图规格）。
const THUMB_MAX: u32 = 256;
/// 缩略图 WebP 有损质量。
const THUMB_QUALITY: u8 = 80;

/// 缩略图服务：列表优先读缓存，失效自动重建。
pub struct ThumbnailService;

impl ThumbnailService {
    /// 确保缩略图存在并返回路径（DB 缓存 + 磁盘缓存 + mtime 校验）。
    pub fn ensure_thumbnail(
        repo: &SqliteReviewRepository,
        image_id: i64,
        source: &Path,
    ) -> ReviewResult<PathBuf> {
        if let Ok(item) = repo.get_image(image_id) {
            if let Some(ref thumb) = item.thumbnail_path {
                if thumb.exists() && !is_stale(source, thumb) {
                    return Ok(thumb.clone());
                }
            }
        }
        let thumb_path = Self::generate(source)?;
        repo.set_thumbnail_path(image_id, &thumb_path)?;
        Ok(thumb_path)
    }

    /// 生成（或复用有效缓存）缩略图，返回缓存路径。
    pub fn generate(source: &Path) -> ReviewResult<PathBuf> {
        let cache_dir = thumbnail_cache_dir()?;
        std::fs::create_dir_all(&cache_dir)?;
        let dest = cache_path(&cache_dir, source);
        if dest.exists() && !is_stale(source, &dest) {
            return Ok(dest);
        }
        let img = image::open(source).map_err(|source_err| ReviewError::ImageDecode {
            path: source.to_path_buf(),
            source: source_err,
        })?;
        let thumb = resize_thumb(&img);
        let quality = Quality::new(THUMB_QUALITY).unwrap_or(Quality::DEFAULT);
        let encoded = encode_dynamic_image(&thumb, ImageFormat::WebP, quality)
            .map_err(|e| ReviewError::Message(format!("缩略图编码失败：{e}")))?;
        std::fs::write(&dest, encoded)?;
        Ok(dest)
    }

    /// 清空全部缩略图缓存，返回删除文件数。
    pub fn clear_cache() -> ReviewResult<usize> {
        let cache_dir = thumbnail_cache_dir()?;
        if !cache_dir.exists() {
            return Ok(0);
        }
        let mut removed = 0usize;
        for entry in std::fs::read_dir(&cache_dir)? {
            let entry = entry?;
            if entry.path().is_file() && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        tracing::info!(removed, "review thumbnail cache cleared");
        Ok(removed)
    }

    /// 若磁盘缓存有效则返回路径（不触发生成）。
    pub fn valid_cache_path(source: &Path) -> Option<PathBuf> {
        let cache_dir = thumbnail_cache_dir().ok()?;
        let dest = cache_path(&cache_dir, source);
        if dest.exists() && !is_stale(source, &dest) {
            Some(dest)
        } else {
            None
        }
    }
}

/// 缓存文件路径：源路径哈希 + 尺寸后缀，WebP 扩展名。
fn cache_path(cache_dir: &Path, source: &Path) -> PathBuf {
    let key = hash_path(source);
    cache_dir.join(format!("{key:016x}_{THUMB_MAX}.webp"))
}

/// 通过源文件修改时间判断缓存是否失效。
fn is_stale(source: &Path, thumb: &Path) -> bool {
    let src_mtime = std::fs::metadata(source).and_then(|m| m.modified()).ok();
    let thumb_mtime = std::fs::metadata(thumb).and_then(|m| m.modified()).ok();
    match (src_mtime, thumb_mtime) {
        (Some(src), Some(cache)) => src > cache,
        _ => false,
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

fn hash_path(path: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    h.finish()
}

/// 后台异步生成磁盘缩略图（不阻塞 UI）。
pub struct AsyncThumbnailGenerator {
    tx: std::sync::mpsc::Sender<(i64, PathBuf)>,
    rx: std::sync::mpsc::Receiver<(i64, PathBuf)>,
}

impl AsyncThumbnailGenerator {
    pub fn new() -> Self {
        use std::sync::mpsc;
        use std::thread;
        let (job_tx, job_rx) = mpsc::channel::<(i64, PathBuf)>();
        let (res_tx, res_rx) = mpsc::channel::<(i64, PathBuf)>();
        thread::spawn(move || {
            while let Ok((id, source)) = job_rx.recv() {
                if let Ok(path) = ThumbnailService::generate(&source) {
                    let _ = res_tx.send((id, path));
                }
            }
        });
        Self {
            tx: job_tx,
            rx: res_rx,
        }
    }

    pub fn request(&self, image_id: i64, source: PathBuf) {
        let _ = self.tx.send((image_id, source));
    }

    pub fn poll(&self) -> Vec<(i64, PathBuf)> {
        use std::sync::mpsc::TryRecvError;
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(v) => out.push(v),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

impl Default for AsyncThumbnailGenerator {
    fn default() -> Self {
        Self::new()
    }
}
