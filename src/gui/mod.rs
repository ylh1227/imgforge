//! 图形界面模块。

mod app;
mod app_types;
mod async_job;
mod fonts;
mod macos;
mod native;
mod quality_preview;
mod task_center;
pub mod theme;
pub mod widgets;

pub use app::ImgforgeApp;
pub use async_job::{BackgroundJob, JobContext};
pub use crate::prefs::{
    ActionHistoryEntry, ActionHistoryStatus, ConvertPresetSnapshot, CustomReviewStatus,
    ExportTemplate, GuiPrefs, ReviewComment, TaskHistoryEntry,
};
/// 兼容旧路径 `crate::gui::prefs`。
pub mod prefs {
    pub use crate::prefs::*;
}
