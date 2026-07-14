//! GUI 应用内部类型与评审宿主适配器。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::review::ui::ReviewPanelHost;
use crate::ui::progress::ProgressReporter;
use crate::ui::report::ProcessReport;

/// 主应用 → 评审面板的转换队列上下文。
pub(crate) struct AppReviewHost<'a> {
    pub(crate) queue: &'a [PathBuf],
    pub(crate) output_dir: &'a str,
}

impl ReviewPanelHost for AppReviewHost<'_> {
    fn conversion_queue_paths(&self) -> &[PathBuf] {
        self.queue
    }

    fn output_directory(&self) -> &str {
        self.output_dir
    }
}

pub(crate) enum RunState {
    Idle,
    Running {
        cancelled: Arc<AtomicBool>,
        progress: Arc<dyn ProgressReporter>,
    },
    Done(ProcessReport),
    Failed,
}

pub(crate) enum WorkerMessage {
    Finished(Result<ProcessReport, String>),
    /// 设备媒体导入完成（手机 / 运动相机等）。
    ImportFinished(Result<DeviceImportResult, String>),
    /// 远端任务已提交（或提交失败）。
    RemoteSubmitted {
        job_id: Option<String>,
        message: String,
        ok: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct DeviceImportResult {
    pub staging_dir: PathBuf,
    pub file_count: usize,
    pub image_count: usize,
    pub video_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppMode {
    Convert,
    Review,
    VideoReview,
    DataExtract,
    TaskCenter,
}
