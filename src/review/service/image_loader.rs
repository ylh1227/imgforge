//! 三级异步图片加载：缩略图 / 预览 / 原图。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use image::GenericImageView;

use crate::review::error::ReviewResult;

/// 加载层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageLoadTier {
    Thumb,
    Preview,
    Full,
}

impl ImageLoadTier {
    pub fn max_edge(self) -> Option<u32> {
        match self {
            Self::Thumb => Some(320),
            Self::Preview => Some(1920),
            Self::Full => None,
        }
    }
}

/// 后台解码完成的图片。
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub path: PathBuf,
    pub key: String,
    pub tier: ImageLoadTier,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 解码失败（用于 UI 停死循环并展示原因）。
#[derive(Debug, Clone)]
pub struct DecodeFailure {
    pub path: PathBuf,
    pub key: String,
    pub tier: ImageLoadTier,
    pub error: String,
}

#[derive(Debug, Clone)]
pub enum LoadOutcome {
    Ok(DecodedImage),
    Err(DecodeFailure),
}

struct LoadJob {
    key: String,
    path: PathBuf,
    thumb: Option<PathBuf>,
    tier: ImageLoadTier,
}

/// 后台线程解码器（不阻塞 UI）。
pub struct AsyncImageLoader {
    tx: Sender<LoadJob>,
    rx: Receiver<LoadOutcome>,
    inflight: Arc<Mutex<VecDeque<String>>>,
}

impl AsyncImageLoader {
    pub fn new(_worker_count: usize) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<LoadJob>();
        let (result_tx, result_rx) = mpsc::channel::<LoadOutcome>();
        let inflight = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let inflight_worker = Arc::clone(&inflight);
        thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let outcome = decode_job(&job);
                let _ = result_tx.send(outcome);
                if let Ok(mut q) = inflight_worker.lock() {
                    q.retain(|k| k != &job.key);
                }
            }
        });
        Self {
            tx: job_tx,
            rx: result_rx,
            inflight,
        }
    }

    pub fn request(
        &self,
        path: &Path,
        thumb: Option<&Path>,
        tier: ImageLoadTier,
    ) -> ReviewResult<()> {
        if is_non_filesystem_path(path) {
            return Ok(());
        }
        let key = cache_key(path, tier);
        if let Ok(q) = self.inflight.lock() {
            if q.iter().any(|k| k == &key) {
                return Ok(());
            }
        }
        if let Ok(mut q) = self.inflight.lock() {
            q.push_back(key.clone());
        }
        let _ = self.tx.send(LoadJob {
            key,
            path: path.to_path_buf(),
            thumb: thumb.map(Path::to_path_buf),
            tier,
        });
        Ok(())
    }

    pub fn poll(&self) -> Vec<LoadOutcome> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(img) => out.push(img),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// 非阻塞取一条解码结果（用于限制每帧上传量）。
    pub fn try_recv_one(&self) -> Option<LoadOutcome> {
        self.rx.try_recv().ok()
    }

    pub fn prefetch_neighbors(
        &self,
        paths: &[PathBuf],
        center: usize,
        radius: usize,
        thumb_paths: &[Option<PathBuf>],
    ) {
        if paths.is_empty() {
            return;
        }
        let start = center.saturating_sub(radius);
        let end = (center + radius).min(paths.len().saturating_sub(1));
        for i in start..=end {
            let thumb = thumb_paths.get(i).and_then(|t| t.as_deref());
            let _ = self.request(&paths[i], thumb, ImageLoadTier::Thumb);
        }
    }
}

/// 远程占位路径等不可直接 `image::open` 的路径。
pub fn is_non_filesystem_path(path: &Path) -> bool {
    path.to_string_lossy().starts_with("remote://")
}

const KEY_SEP: char = '\u{1e}';

pub fn cache_key(path: &Path, tier: ImageLoadTier) -> String {
    format!("{}{KEY_SEP}{tier:?}", path.to_string_lossy())
}

fn decode_job(job: &LoadJob) -> LoadOutcome {
    if is_non_filesystem_path(&job.path) {
        return LoadOutcome::Err(DecodeFailure {
            path: job.path.clone(),
            key: job.key.clone(),
            tier: job.tier,
            error: "原图尚未下载到本地".into(),
        });
    }

    let load_path = job
        .thumb
        .as_deref()
        .filter(|p| p.exists() && !is_non_filesystem_path(p) && job.tier == ImageLoadTier::Thumb)
        .unwrap_or(&job.path);

    if !load_path.exists() {
        return LoadOutcome::Err(DecodeFailure {
            path: job.path.clone(),
            key: job.key.clone(),
            tier: job.tier,
            error: format!("文件不存在：{}", load_path.display()),
        });
    }

    let img = match image::open(load_path) {
        Ok(img) => img,
        Err(e) => {
            return LoadOutcome::Err(DecodeFailure {
                path: job.path.clone(),
                key: job.key.clone(),
                tier: job.tier,
                error: format!("无法解码：{e}"),
            });
        }
    };

    let (mut w, mut h) = img.dimensions();
    if let Some(max_edge) = job.tier.max_edge() {
        let max_dim = w.max(h);
        if max_dim > max_edge {
            let scale = max_edge as f32 / max_dim as f32;
            w = ((w as f32 * scale).round() as u32).max(1);
            h = ((h as f32 * scale).round() as u32).max(1);
            let resized = img.resize_exact(w, h, image::imageops::FilterType::Triangle);
            let rgba = resized.to_rgba8();
            return LoadOutcome::Ok(DecodedImage {
                path: job.path.clone(),
                key: job.key.clone(),
                tier: job.tier,
                width: w,
                height: h,
                rgba: rgba.into_raw(),
            });
        }
    }
    let rgba = img.to_rgba8();
    LoadOutcome::Ok(DecodedImage {
        path: job.path.clone(),
        key: job.key.clone(),
        tier: job.tier,
        width: w,
        height: h,
        rgba: rgba.into_raw(),
    })
}
