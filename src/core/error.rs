//! 业务错误枚举与统一 Result 别名。

use std::path::PathBuf;
use thiserror::Error;

/// 应用级错误枚举，覆盖 IO、编解码、配置与流水线场景。
#[derive(Debug, Error)]
pub enum AppError {
  #[error("IO error at {path}: {source}")]
  Io {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },

  #[error("failed to decode image at {path}: {reason}")]
  DecodeFailed { path: PathBuf, reason: String },

  #[error("failed to encode image to {format}: {reason}")]
  EncodeFailed { format: String, reason: String },

  #[error("unsupported image format: {0}")]
  UnsupportedFormat(String),

  #[error("configuration error: {0}")]
  Config(String),

  #[error("invalid quality value {0}: must be between 1 and 100")]
  InvalidQuality(u8),

  #[error("invalid concurrency value {0}: must be at least 1")]
  InvalidConcurrency(usize),

  #[error("pipeline error at step '{step}': {reason}")]
  Pipeline { step: String, reason: String },

  #[error("path traversal detected: {0}")]
  PathTraversal(PathBuf),

  #[error("incremental processing error: {0}")]
  Incremental(String),

  #[error("task cancelled")]
  Cancelled,

  #[error("no input files matched the given filters")]
  NoInputFiles,

  #[error("{0}")]
  Other(String),
}

impl AppError {
  pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
    Self::Io {
      path: path.into(),
      source,
    }
  }
}

/// 应用统一 Result 类型。
pub type AppResult<T> = Result<T, AppError>;
