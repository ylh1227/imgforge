//! 视频评审模块错误类型。

use std::path::PathBuf;

use thiserror::Error;

pub type VideoReviewResult<T> = Result<T, VideoReviewError>;

#[derive(Debug, Error)]
pub enum VideoReviewError {
  #[error("数据库错误：{0}")]
  Database(#[from] rusqlite::Error),

  #[error("JSON 错误：{0}")]
  Json(#[from] serde_json::Error),

  #[error("CSV 错误：{0}")]
  Csv(#[from] csv::Error),

  #[error("IO 错误：{0}")]
  Io(#[from] std::io::Error),

  #[error("记录不存在：{entity} id={id}")]
  NotFound { entity: &'static str, id: i64 },

  #[error("ffmpeg 不可用：{0}")]
  FfmpegUnavailable(String),

  #[error("ffprobe 失败 {path}: {detail}")]
  FfprobeFailed { path: PathBuf, detail: String },

  #[error("ffmpeg 抽帧失败 {path}: {detail}")]
  FrameExtractFailed { path: PathBuf, detail: String },

  #[error("视频导出失败：{detail}")]
  VideoExportFailed { detail: String },

  #[error("{0}")]
  Message(String),
}
