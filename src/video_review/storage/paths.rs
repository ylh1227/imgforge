//! 视频评审数据目录。

use std::path::PathBuf;

use crate::review::storage::paths::app_data_dir;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};

pub fn video_frame_cache_dir() -> VideoReviewResult<PathBuf> {
  Ok(app_data_dir().map_err(|e| VideoReviewError::Message(e.to_string()))?.join("video_frames"))
}

pub fn video_config_path() -> VideoReviewResult<PathBuf> {
  Ok(
    app_data_dir()
      .map_err(|e| VideoReviewError::Message(e.to_string()))?
      .join("video_review_config.json"),
  )
}
