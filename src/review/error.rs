//! 评审模块专属错误类型。

use std::path::PathBuf;

use thiserror::Error;

pub type ReviewResult<T> = Result<T, ReviewError>;

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error("数据库错误：{0}")]
    Database(#[from] rusqlite::Error),

    #[error("图片解码失败 {path}: {source}")]
    ImageDecode {
        path: PathBuf,
        source: image::ImageError,
    },

    #[error("图片编码失败：{0}")]
    ImageEncode(#[from] image::ImageError),

    #[error("JSON 序列化错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("CSV 错误：{0}")]
    Csv(#[from] csv::Error),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("记录不存在：{entity} id={id}")]
    NotFound { entity: &'static str, id: i64 },

    #[error("无效路径：{0}")]
    InvalidPath(PathBuf),

    #[error("批次为空")]
    EmptyBatch,

    #[error("无当前选中图片")]
    NoSelection,

    #[error("{0}")]
    Message(String),
}
