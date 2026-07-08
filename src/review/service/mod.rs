//! 业务编排层。

mod analysis_service;
mod batch_operations;
mod batch_service;
mod config;
pub mod conversion_bridge;
mod export_service;
mod image_loader;
mod review_service;
mod screenshot_service;
mod shortcuts;
mod thumbnail_service;

pub use analysis_service::{ImageAnalysis, ImageAnalysisService};
pub use batch_operations::{
    is_irreversible_transition, BatchAnnotateRequest, BatchAnnotateResult, BatchItemFailure,
    BatchOperations, BatchRemarkRequest, BatchRemarkResult, BatchStatusRequest, BatchStatusResult,
    StatusTransitionWarning,
};
pub use batch_service::BatchService;
pub use config::ReviewModuleConfig;
pub use conversion_bridge::{ConversionTaskParams, ReviewConversionBridge, ReviewQueueItem};
pub use export_service::{
    BatchJsonExportRequest, CsvExportRequest, CsvExportResult, ExportService, JsonSidecarRequest,
};
pub use image_loader::{cache_key, AsyncImageLoader, DecodedImage, ImageLoadTier};
pub use review_service::ReviewService;
pub use screenshot_service::{
    BatchImageScreenshotRequest, BatchImageScreenshotResult, BatchImageScreenshotService,
    ImageScreenshotManifestEntry,
};
pub use shortcuts::{save_custom_binding, ShortcutAction, ShortcutConfig};
pub use thumbnail_service::{AsyncThumbnailGenerator, ThumbnailService};
