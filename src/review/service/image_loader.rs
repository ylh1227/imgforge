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
  pub key: String,
  pub tier: ImageLoadTier,
  pub width: u32,
  pub height: u32,
  pub rgba: Vec<u8>,
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
  rx: Receiver<DecodedImage>,
  inflight: Arc<Mutex<VecDeque<String>>>,
}

impl AsyncImageLoader {
  pub fn new(_worker_count: usize) -> Self {
    let (job_tx, job_rx) = mpsc::channel::<LoadJob>();
    let (result_tx, result_rx) = mpsc::channel::<DecodedImage>();
    let inflight = Arc::new(Mutex::new(VecDeque::<String>::new()));
    let inflight_worker = Arc::clone(&inflight);
    thread::spawn(move || {
      while let Ok(job) = job_rx.recv() {
        if let Some(img) = decode_job(&job) {
          let _ = result_tx.send(img);
        }
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

  pub fn poll(&self) -> Vec<DecodedImage> {
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
  pub fn try_recv_one(&self) -> Option<DecodedImage> {
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

pub fn cache_key(path: &Path, tier: ImageLoadTier) -> String {
  format!("{}:{tier:?}", path.to_string_lossy())
}

fn decode_job(job: &LoadJob) -> Option<DecodedImage> {
  let load_path = job
    .thumb
    .as_deref()
    .filter(|p| p.exists() && job.tier == ImageLoadTier::Thumb)
    .unwrap_or(&job.path);
  let img = image::open(load_path).ok()?;
  let (mut w, mut h) = img.dimensions();
  if let Some(max_edge) = job.tier.max_edge() {
    let max_dim = w.max(h);
    if max_dim > max_edge {
      let scale = max_edge as f32 / max_dim as f32;
      w = ((w as f32 * scale).round() as u32).max(1);
      h = ((h as f32 * scale).round() as u32).max(1);
      let resized = img.resize_exact(w, h, image::imageops::FilterType::Triangle);
      let rgba = resized.to_rgba8();
      return Some(DecodedImage {
        key: job.key.clone(),
        tier: job.tier,
        width: w,
        height: h,
        rgba: rgba.into_raw(),
      });
    }
  }
  let rgba = img.to_rgba8();
  Some(DecodedImage {
    key: job.key.clone(),
    tier: job.tier,
    width: w,
    height: h,
    rgba: rgba.into_raw(),
  })
}
