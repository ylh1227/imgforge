//! 视频评审模块：批次导入、ffprobe 元数据、ffmpeg 抽帧、多视频同步对比与导出。

pub mod domain;
pub mod error;
pub mod service;
pub mod storage;

#[cfg(feature = "gui")]
pub mod ui;

pub use error::{VideoReviewError, VideoReviewResult};
pub use service::VideoReviewService;

#[cfg(feature = "gui")]
pub use ui::{VideoReviewPanel, VideoReviewPanelOutput};
