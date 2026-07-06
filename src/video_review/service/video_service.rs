//! 视频评审业务服务。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jwalk::WalkDir;

use crate::video_review::domain::{
  is_video_extension, BatchStats, MarkerKind, VideoBatch, VideoFilter, VideoItem, VideoMarker,
  VideoSegment, VideoTag,
};
use crate::review::domain::image_item::ReviewStatus;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::contact_sheet::{ContactSheetResult, FrameProvider};
use crate::video_review::service::export_service::{ContactSheetExportRequest, VideoExportService};
use crate::video_review::service::grid_video::{
  GridVideoExportQuality, GridVideoExportRequest, GridVideoExportResult,
};
use crate::video_review::service::ffmpeg_backend::{FfmpegAvailability, FfmpegBackend, VideoBackend};
use crate::video_review::service::frame_cache::{FrameCache, FrameCacheStats};
use crate::video_review::storage::{NewVideoItem, SqliteVideoRepository, VideoRepository};

pub struct VideoReviewService {
  repo: SqliteVideoRepository,
  backend: Arc<dyn VideoBackend>,
  frame_cache: FrameCache,
}

impl VideoReviewService {
  pub fn open() -> VideoReviewResult<Self> {
    let backend: Arc<dyn VideoBackend> = Arc::new(FfmpegBackend::with_defaults());
    let frame_cache = FrameCache::new(backend.clone())?;
    Ok(Self {
      repo: SqliteVideoRepository::open()?,
      backend,
      frame_cache,
    })
  }

  pub fn availability(&self) -> FfmpegAvailability {
    self.backend.availability()
  }

  pub fn repo(&self) -> &SqliteVideoRepository {
    &self.repo
  }

  pub fn frame_cache(&self) -> &FrameCache {
    &self.frame_cache
  }

  pub fn list_batches(&self) -> VideoReviewResult<Vec<VideoBatch>> {
    self.repo.list_batches()
  }

  pub fn batch_stats(&self, batch_id: i64) -> VideoReviewResult<BatchStats> {
    self.repo.batch_stats(batch_id)
  }

  pub fn list_videos(&self, batch_id: i64, filter: &VideoFilter) -> VideoReviewResult<Vec<VideoItem>> {
    self.repo.list_videos(batch_id, filter)
  }

  pub fn get_video(&self, id: i64) -> VideoReviewResult<VideoItem> {
    self.repo.get_video(id)
  }

  pub fn import_folder(&self, folder: &Path, batch_name: Option<&str>) -> VideoReviewResult<i64> {
    let name = batch_name
      .map(str::to_string)
      .or_else(|| folder.file_name().map(|n| n.to_string_lossy().to_string()))
      .unwrap_or_else(|| "未命名批次".into());

    let paths = collect_video_paths(folder)?;
    if paths.is_empty() {
      return Err(VideoReviewError::Message("文件夹内未找到支持的视频文件".into()));
    }

    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
      let meta = match self.backend.probe_metadata(&path) {
        Ok(m) => m,
        Err(e) => {
          tracing::warn!("跳过 {}: {}", path.display(), e);
          continue;
        }
      };
      let thumb = self
        .frame_cache
        .ensure_frame(&path, 0, 320)
        .ok()
        .map(|p| p);
      items.push(NewVideoItem {
        file_path: path,
        thumbnail_path: thumb,
        duration_ms: meta.duration_ms,
        fps: meta.fps,
        width: meta.width,
        height: meta.height,
        video_codec: meta.video_codec,
        audio_codec: meta.audio_codec,
        bitrate_kbps: meta.bitrate_kbps,
      });
    }

    if items.is_empty() {
      return Err(VideoReviewError::Message(
        "未能读取任何视频元数据，请确认已安装 ffprobe".into(),
      ));
    }

    self.repo.create_batch_with_videos(&name, &items)
  }

  pub fn update_status(&self, id: i64, status: ReviewStatus) -> VideoReviewResult<()> {
    self.repo.update_video_status(id, status)
  }

  pub fn update_remark(&self, id: i64, remark: &str) -> VideoReviewResult<()> {
    self.repo.update_video_remark(id, remark)
  }

  pub fn update_offset(&self, id: i64, offset_ms: i64) -> VideoReviewResult<()> {
    self.repo.update_video_offset(id, offset_ms)
  }

  pub fn list_tags(&self) -> VideoReviewResult<Vec<VideoTag>> {
    self.repo.list_tags()
  }

  pub fn create_tag(&self, name: &str, color: [u8; 4]) -> VideoReviewResult<i64> {
    self.repo.create_tag(name, color)
  }

  pub fn set_video_tags(&self, video_id: i64, tag_ids: &[i64]) -> VideoReviewResult<()> {
    self.repo.set_video_tags(video_id, tag_ids)
  }

  pub fn get_video_tag_ids(&self, video_id: i64) -> VideoReviewResult<Vec<i64>> {
    self.repo.get_video_tag_ids(video_id)
  }

  pub fn add_marker(
    &self,
    video_id: i64,
    time_ms: u64,
    kind: MarkerKind,
    text: &str,
    severity: u8,
  ) -> VideoReviewResult<i64> {
    self.repo.add_marker(video_id, time_ms, kind, text, severity)
  }

  pub fn list_markers(&self, video_id: i64) -> VideoReviewResult<Vec<VideoMarker>> {
    self.repo.list_markers(video_id)
  }

  pub fn delete_marker(&self, id: i64) -> VideoReviewResult<()> {
    self.repo.delete_marker(id)
  }

  pub fn add_segment(
    &self,
    video_id: i64,
    start_ms: u64,
    end_ms: u64,
    text: &str,
    status: ReviewStatus,
  ) -> VideoReviewResult<i64> {
    self.repo.add_segment(video_id, start_ms, end_ms, text, status)
  }

  pub fn list_segments(&self, video_id: i64) -> VideoReviewResult<Vec<VideoSegment>> {
    self.repo.list_segments(video_id)
  }

  pub fn delete_segment(&self, id: i64) -> VideoReviewResult<()> {
    self.repo.delete_segment(id)
  }

  pub fn frame_at(
    &self,
    video: &VideoItem,
    global_time_ms: u64,
    width: u32,
  ) -> VideoReviewResult<Option<PathBuf>> {
    let t = video.effective_time_ms(global_time_ms).min(video.duration_ms);
    self
      .frame_cache
      .get_or_request(&video.file_path, t, width)
  }

  pub fn ensure_cover(&self, video: &VideoItem) -> VideoReviewResult<PathBuf> {
    self.frame_cache.ensure_frame(&video.file_path, 0, 480)
  }

  pub fn timeline_thumbs(
    &self,
    video: &VideoItem,
    count: usize,
  ) -> VideoReviewResult<Vec<(u64, Option<PathBuf>)>> {
    let count = count.clamp(4, 24);
    if video.duration_ms == 0 {
      return Ok(Vec::new());
    }
    let step = video.duration_ms / count as u64;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
      let t = (step * i as u64).min(video.duration_ms.saturating_sub(1));
      let path = self.frame_at(video, t, 160).ok().flatten();
      out.push((t, path));
    }
    Ok(out)
  }

  pub fn ensure_frame_sync(
    &self,
    video: &VideoItem,
    global_time_ms: u64,
    width: u32,
  ) -> VideoReviewResult<PathBuf> {
    let t = video.effective_time_ms(global_time_ms).min(video.duration_ms);
    self.frame_cache.ensure_frame(&video.file_path, t, width)
  }

  pub fn export_compare_contact_sheet(
    &self,
    videos: &[VideoItem],
    time_ms: u64,
    dest: PathBuf,
  ) -> VideoReviewResult<ContactSheetResult> {
    VideoExportService::export_contact_sheet(
      self,
      &ContactSheetExportRequest {
        videos: videos.to_vec(),
        time_ms,
        dest,
      },
    )
  }

  pub fn export_compare_grid_video(
    &self,
    videos: &[VideoItem],
    start_time_ms: u64,
    duration_ms: u64,
    dest: PathBuf,
    quality: GridVideoExportQuality,
  ) -> VideoReviewResult<GridVideoExportResult> {
    VideoExportService::export_grid_video(&GridVideoExportRequest {
      videos: videos.to_vec(),
      start_time_ms,
      duration_ms,
      dest,
      cell_width: 0,
      cell_height: 0,
      quality,
    })
  }

  pub fn batch_update_status(&self, ids: &[i64], status: ReviewStatus) -> VideoReviewResult<()> {
    self.repo.batch_update_status(ids, status)
  }

  pub fn batch_append_remark(&self, ids: &[i64], text: &str) -> VideoReviewResult<()> {
    self.repo.batch_append_remark(ids, text)
  }

  pub fn batch_set_tags(&self, ids: &[i64], tag_ids: &[i64]) -> VideoReviewResult<()> {
    self.repo.batch_set_tags(ids, tag_ids)
  }

  pub fn frame_cache_stats(&self) -> VideoReviewResult<FrameCacheStats> {
    self.frame_cache.stats()
  }

  pub fn clear_frame_cache(&self) -> VideoReviewResult<usize> {
    self.frame_cache.clear()
  }
}

impl FrameProvider for VideoReviewService {
  fn ensure_frame(
    &self,
    video: &VideoItem,
    global_time_ms: u64,
    width: u32,
  ) -> VideoReviewResult<PathBuf> {
    self.ensure_frame_sync(video, global_time_ms, width)
  }
}

fn collect_video_paths(folder: &Path) -> VideoReviewResult<Vec<PathBuf>> {
  let mut paths = Vec::new();
  for entry in WalkDir::new(folder) {
    let entry = match entry {
      Ok(e) => e,
      Err(_) => continue,
    };
    if !entry.file_type().is_file() {
      continue;
    }
    let path = entry.path();
    if path
      .extension()
      .and_then(|e| e.to_str())
      .is_some_and(is_video_extension)
    {
      paths.push(path);
    }
  }
  paths.sort();
  Ok(paths)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::tempdir;

  #[test]
  fn collect_video_paths_filters() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.mp4"), b"").unwrap();
    fs::write(dir.path().join("b.txt"), b"").unwrap();
    fs::write(dir.path().join("c.MOV"), b"").unwrap();
    let paths = collect_video_paths(dir.path()).unwrap();
    assert_eq!(paths.len(), 2);
  }
}
