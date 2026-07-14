//! 图形界面模块。

mod app;
mod app_types;
mod async_job;
mod fonts;
mod macos;
mod native;
pub mod prefs;
mod quality_preview;
mod task_center;
pub mod theme;
pub mod widgets;

pub use app::ImgforgeApp;
pub use async_job::{BackgroundJob, JobContext};
pub use prefs::{
    ActionHistoryEntry, ActionHistoryStatus, ConvertPresetSnapshot, CustomReviewStatus,
    ExportTemplate, GuiPrefs, ReviewComment, TaskHistoryEntry,
};
