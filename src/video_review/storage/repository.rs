//! 视频评审仓储 trait。

use std::path::{Path, PathBuf};

use crate::review::domain::image_item::ReviewStatus;
use crate::video_review::domain::{
  BatchStats, MarkerKind, VideoBatch, VideoFilter, VideoItem, VideoMarker, VideoSegment, VideoTag,
};
use crate::video_review::error::VideoReviewResult;

pub struct NewVideoItem {
  pub file_path: PathBuf,
  pub thumbnail_path: Option<PathBuf>,
  pub duration_ms: u64,
  pub fps: f32,
  pub width: u32,
  pub height: u32,
  pub video_codec: String,
  pub audio_codec: Option<String>,
  pub bitrate_kbps: Option<u32>,
}

pub trait VideoRepository {
  fn create_batch(&self, name: &str) -> VideoReviewResult<i64>;
  fn list_batches(&self) -> VideoReviewResult<Vec<VideoBatch>>;
  fn get_batch(&self, id: i64) -> VideoReviewResult<VideoBatch>;
  fn delete_batch(&self, id: i64) -> VideoReviewResult<()>;

  fn add_videos(&self, batch_id: i64, items: &[NewVideoItem]) -> VideoReviewResult<()>;
  fn list_videos(&self, batch_id: i64, filter: &VideoFilter) -> VideoReviewResult<Vec<VideoItem>>;
  fn get_video(&self, id: i64) -> VideoReviewResult<VideoItem>;
  fn update_video_status(&self, id: i64, status: ReviewStatus) -> VideoReviewResult<()>;
  fn update_video_remark(&self, id: i64, remark: &str) -> VideoReviewResult<()>;
  fn update_video_offset(&self, id: i64, offset_ms: i64) -> VideoReviewResult<()>;
  fn set_thumbnail_path(&self, id: i64, path: &Path) -> VideoReviewResult<()>;
  fn batch_stats(&self, batch_id: i64) -> VideoReviewResult<BatchStats>;

  fn list_tags(&self) -> VideoReviewResult<Vec<VideoTag>>;
  fn create_tag(&self, name: &str, color: [u8; 4]) -> VideoReviewResult<i64>;
  fn delete_tag(&self, id: i64) -> VideoReviewResult<()>;
  fn get_video_tag_ids(&self, video_id: i64) -> VideoReviewResult<Vec<i64>>;
  fn set_video_tags(&self, video_id: i64, tag_ids: &[i64]) -> VideoReviewResult<()>;
  fn batch_set_tags(&self, video_ids: &[i64], tag_ids: &[i64]) -> VideoReviewResult<()>;
  fn batch_update_status(&self, ids: &[i64], status: ReviewStatus) -> VideoReviewResult<()>;
  fn batch_append_remark(&self, ids: &[i64], text: &str) -> VideoReviewResult<()>;

  fn add_marker(
    &self,
    video_id: i64,
    time_ms: u64,
    kind: MarkerKind,
    text: &str,
    severity: u8,
  ) -> VideoReviewResult<i64>;
  fn list_markers(&self, video_id: i64) -> VideoReviewResult<Vec<VideoMarker>>;
  fn delete_marker(&self, id: i64) -> VideoReviewResult<()>;

  fn add_segment(
    &self,
    video_id: i64,
    start_ms: u64,
    end_ms: u64,
    text: &str,
    status: ReviewStatus,
  ) -> VideoReviewResult<i64>;
  fn list_segments(&self, video_id: i64) -> VideoReviewResult<Vec<VideoSegment>>;
  fn delete_segment(&self, id: i64) -> VideoReviewResult<()>;

  fn save_session_value(&self, key: &str, value: &str) -> VideoReviewResult<()>;
  fn load_session_value(&self, key: &str) -> VideoReviewResult<Option<String>>;
}
