//! 视频抽帧磁盘缓存。

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::video_review::error::VideoReviewResult;
use crate::video_review::service::ffmpeg_backend::VideoBackend;
use crate::video_review::storage::paths::video_frame_cache_dir;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    path_hash: u64,
    mtime: u64,
    time_ms: u64,
    width: u32,
}

#[derive(Debug, Clone, Default)]
pub struct FrameCacheStats {
    pub file_count: usize,
    pub total_bytes: u64,
    pub pending_count: usize,
}

pub struct FrameCache {
    cache_dir: PathBuf,
    backend: Arc<dyn VideoBackend>,
    pending: Mutex<std::collections::HashSet<CacheKey>>,
    worker_tx: Sender<FrameJob>,
}

struct FrameJob {
    key: CacheKey,
    video_path: PathBuf,
    output: PathBuf,
    result_tx: Sender<VideoReviewResult<PathBuf>>,
}

impl FrameCache {
    pub fn new(backend: Arc<dyn VideoBackend>) -> VideoReviewResult<Self> {
        let cache_dir = video_frame_cache_dir()?;
        fs::create_dir_all(&cache_dir)?;
        let (worker_tx, worker_rx) = mpsc::channel::<FrameJob>();
        let backend_worker = backend.clone();
        thread::spawn(move || Self::worker_loop(worker_rx, backend_worker));

        Ok(Self {
            cache_dir,
            backend,
            pending: Mutex::new(std::collections::HashSet::new()),
            worker_tx,
        })
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn stats(&self) -> VideoReviewResult<FrameCacheStats> {
        let mut stats = FrameCacheStats::default();
        stats.pending_count = self.pending.lock().unwrap().len();
        if !self.cache_dir.exists() {
            return Ok(stats);
        }
        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                stats.file_count += 1;
                stats.total_bytes += entry.metadata()?.len();
            }
        }
        Ok(stats)
    }

    pub fn clear(&self) -> VideoReviewResult<usize> {
        let mut removed = 0usize;
        if !self.cache_dir.exists() {
            return Ok(0);
        }
        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// 异步请求抽帧；若缓存命中立即返回，否则后台抽帧并尽量等待一次结果。
    pub fn get_or_request(
        &self,
        video_path: &Path,
        time_ms: u64,
        width: u32,
    ) -> VideoReviewResult<Option<PathBuf>> {
        let key = self.make_key(video_path, time_ms, width)?;
        let out = self.output_path(&key);
        if out.exists() {
            return Ok(Some(out));
        }

        let mut pending = self.pending.lock().unwrap();
        if pending.contains(&key) {
            return Ok(None);
        }
        pending.insert(key.clone());
        drop(pending);

        let (result_tx, result_rx) = mpsc::channel();
        let job = FrameJob {
            key: key.clone(),
            video_path: video_path.to_path_buf(),
            output: out.clone(),
            result_tx,
        };
        let _ = self.worker_tx.send(job);
        if let Ok(Ok(path)) = result_rx.recv() {
            let mut pending = self.pending.lock().unwrap();
            pending.remove(&key);
            return Ok(Some(path));
        }
        let mut pending = self.pending.lock().unwrap();
        pending.remove(&key);
        Ok(None)
    }

    /// 同步抽帧（导出、contact sheet 使用）。
    pub fn ensure_frame(
        &self,
        video_path: &Path,
        time_ms: u64,
        width: u32,
    ) -> VideoReviewResult<PathBuf> {
        let key = self.make_key(video_path, time_ms, width)?;
        let out = self.output_path(&key);
        if out.exists() {
            return Ok(out);
        }
        self.backend
            .extract_frame(video_path, time_ms, width, &out)?;
        Ok(out)
    }

    /// 顺序同步抽帧，限制并发，适合批量导出。
    pub fn ensure_frames_sequential(
        &self,
        frames: &[(PathBuf, u64, u32)],
    ) -> VideoReviewResult<Vec<PathBuf>> {
        let mut out = Vec::with_capacity(frames.len());
        for (path, time_ms, width) in frames {
            out.push(self.ensure_frame(path, *time_ms, *width)?);
        }
        Ok(out)
    }

    fn worker_loop(rx: Receiver<FrameJob>, backend: Arc<dyn VideoBackend>) {
        while let Ok(job) = rx.recv() {
            let result = if job.output.exists() {
                Ok(job.output.clone())
            } else {
                backend
                    .extract_frame(&job.video_path, job.key.time_ms, job.key.width, &job.output)
                    .map(|_| job.output.clone())
            };
            let _ = job.result_tx.send(result);
        }
    }

    fn make_key(&self, video_path: &Path, time_ms: u64, width: u32) -> VideoReviewResult<CacheKey> {
        let meta = fs::metadata(video_path)?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(CacheKey {
            path_hash: cache_key_hash(video_path, mtime, time_ms, width),
            mtime,
            time_ms,
            width,
        })
    }

    fn output_path(&self, key: &CacheKey) -> PathBuf {
        self.cache_dir.join(format!(
            "{}_{}_{}_{}.jpg",
            key.path_hash, key.mtime, key.time_ms, key.width
        ))
    }
}

pub fn cache_key_hash(video_path: &Path, mtime: u64, time_ms: u64, width: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    video_path.to_string_lossy().hash(&mut hasher);
    mtime.hash(&mut hasher);
    time_ms.hash(&mut hasher);
    width.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_stable() {
        let a = cache_key_hash(Path::new("/tmp/v.mp4"), 100, 500, 320);
        let b = cache_key_hash(Path::new("/tmp/v.mp4"), 100, 500, 320);
        assert_eq!(a, b);
        let c = cache_key_hash(Path::new("/tmp/v.mp4"), 100, 501, 320);
        assert_ne!(a, c);
    }
}
