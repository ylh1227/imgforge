//! 轻量本地图片评审模块：批次管理、标注、状态持久化，与格式转换低耦合。

pub mod domain;
pub mod error;
pub mod service;
pub mod storage;

#[cfg(feature = "gui")]
pub mod ui;

pub use error::{ReviewError, ReviewResult};
pub use service::{
  BatchAnnotateRequest, BatchAnnotateResult, BatchItemFailure, BatchJsonExportRequest,
  BatchOperations, BatchRemarkRequest, BatchRemarkResult, BatchStatusRequest,
  BatchStatusResult, CsvExportRequest, CsvExportResult, ExportService, JsonSidecarRequest,
  ReviewConversionBridge, ReviewQueueItem, ReviewService, ShortcutAction, ShortcutConfig,
  StatusTransitionWarning, is_irreversible_transition,
};
pub use storage::traits::{AnnotationTemplate, RemarkWriteMode, ReviewExportRow, ReviewStorage};
