//! 视频条目。

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::review::domain::image_item::ReviewStatus;

use super::metadata::VideoMetadata;

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v"];

#[derive(Debug, Clone)]
pub struct VideoItem {
  pub id: i64,
  pub batch_id: i64,
  pub file_path: PathBuf,
  pub status: ReviewStatus,
  pub remark: Option<String>,
  pub thumbnail_path: Option<PathBuf>,
  pub duration_ms: u64,
  pub fps: f32,
  pub width: u32,
  pub height: u32,
  pub video_codec: String,
  pub audio_codec: Option<String>,
  pub bitrate_kbps: Option<u32>,
  pub offset_ms: i64,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub deleted_at: Option<DateTime<Utc>>,
}

impl VideoItem {
  pub fn metadata(&self) -> VideoMetadata {
    VideoMetadata {
      duration_ms: self.duration_ms,
      fps: self.fps,
      width: self.width,
      height: self.height,
      video_codec: self.video_codec.clone(),
      audio_codec: self.audio_codec.clone(),
      bitrate_kbps: self.bitrate_kbps,
    }
  }

  pub fn is_deleted(&self) -> bool {
    self.deleted_at.is_some()
  }

  pub fn effective_time_ms(&self, global_ms: u64) -> u64 {
    let shifted = global_ms as i64 + self.offset_ms;
    shifted.max(0) as u64
  }
}

#[derive(Debug, Clone, Default)]
pub struct VideoFilter {
  pub status: Option<ReviewStatus>,
  pub search: String,
  pub tag_ids: Vec<i64>,
  pub include_deleted: bool,
}

impl VideoFilter {
  pub fn reset(&mut self) {
    *self = Self::default();
  }

  pub fn apply_in_memory(&self, items: &mut Vec<VideoItem>) {
    if let Some(status) = self.status {
      items.retain(|i| i.status == status);
    }
    if !self.search.is_empty() {
      let q = self.search.to_lowercase();
      items.retain(|i| {
        i.file_path
          .file_name()
          .and_then(|n| n.to_str())
          .unwrap_or("")
          .to_lowercase()
          .contains(&q)
      });
    }
  }
}

pub fn is_video_extension(ext: &str) -> bool {
  VIDEO_EXTENSIONS
    .iter()
    .any(|e| e.eq_ignore_ascii_case(ext))
}
