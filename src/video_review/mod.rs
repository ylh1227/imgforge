//! 视频评审模块：批次导入、ffprobe 元数据、ffmpeg 抽帧、多视频同步对比与导出。

pub mod domain;
pub mod error;
pub mod scene_recognize;
pub mod scopes;
pub mod service;
pub mod storage;

#[cfg(feature = "gui")]
pub mod playback;

#[cfg(feature = "gui")]
pub mod ui;

pub use error::{VideoReviewError, VideoReviewResult};
pub use scene_recognize::{
    recognize_and_rename_video_batch, VideoRecognizeBatchReport, VideoRecognizeItemResult,
};
pub use service::VideoReviewService;

#[cfg(feature = "gui")]
pub use ui::{VideoReviewPanel, VideoReviewPanelOutput};
