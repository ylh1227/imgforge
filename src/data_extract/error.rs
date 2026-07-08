//! 数据提取模块错误类型。

use std::path::PathBuf;

use thiserror::Error;

pub type DataExtractResult<T> = Result<T, DataExtractError>;

#[derive(Debug, Error)]
pub enum DataExtractError {
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("CSV 错误：{0}")]
    Csv(#[from] csv::Error),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("解析失败 {path}: {detail}")]
    ParseFailed { path: PathBuf, detail: String },

    #[error("{0}")]
    Message(String),
}
