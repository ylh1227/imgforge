//! 轻量本地图片评审模块：批次管理、标注、状态持久化，与格式转换低耦合。

pub mod domain;
pub mod error;
pub mod service;
pub mod storage;

#[cfg(feature = "gui")]
pub mod ui;

pub use domain::{
    ConvertParams, CustomStatusLabel, ImageMetadata, ImageSortKey, ReviewTag, TagFilterMode,
};
pub use error::{ReviewError, ReviewResult};
pub use service::{
    is_irreversible_transition, save_custom_binding, BatchAnnotateRequest, BatchAnnotateResult,
    BatchItemFailure, BatchJsonExportRequest, BatchOperations, BatchRemarkRequest,
    BatchRemarkResult, BatchStatusRequest, BatchStatusResult, ConversionTaskParams,
    CsvExportRequest, CsvExportResult, ExportService, JsonSidecarRequest, ReviewConversionBridge,
    ReviewModuleConfig, ReviewQueueItem, ReviewService, ShortcutAction, ShortcutConfig,
    StatusTransitionWarning,
};
pub use storage::traits::{AnnotationTemplate, RemarkWriteMode, ReviewExportRow, ReviewStorage};
pub use storage::{create_backup, list_backups, restore_backup};
