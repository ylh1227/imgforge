//! 视频评审服务层。

pub mod align_service;
pub mod analysis_service;
pub mod contact_sheet;
pub mod defect_package;
pub mod export_service;
pub mod ffmpeg_backend;
pub mod frame_cache;
pub mod grid_video;
pub mod screenshot_service;
pub mod video_service;

pub use align_service::{
    offset_after_frame_step, quantize_offset_to_fps, AlignBatchResult, AlignPairResult,
    AlignService, DEFAULT_ALIGN_SECONDS,
};
pub use analysis_service::{VideoAnalysisService, VideoAnalysisSuggestion};
pub use contact_sheet::{
    compute_layout, grid_dimensions, ContactSheetRequest, ContactSheetResult, ContactSheetService,
    FrameProvider, GridLayout,
};
pub use defect_package::{
    create_defect_package, default_defect_output_dir, CreateDefectRequest, CreateDefectResult,
    ALIGN_CONFIDENCE_WARN, DEFAULT_DEFECT_HALF_WINDOW_MS,
};
pub use export_service::{
    ContactSheetExportRequest, ContactSheetMeta, VideoExportColumn, VideoExportRequest,
    VideoExportResult, VideoExportRow, VideoExportSchema, VideoExportService,
};
pub use ffmpeg_backend::{
    parse_ffprobe_json, parse_frame_rate, FfmpegAvailability, FfmpegBackend, FfmpegConfig,
    VideoBackend,
};
pub use frame_cache::FrameCache;
pub use grid_video::{
    compute_quality_cell_size, max_export_duration_ms, GridVideoCaptionMode,
    GridVideoExportQuality, GridVideoExportRequest, GridVideoExportResult, GridVideoExportService,
    DEFAULT_CELL_HEIGHT, DEFAULT_CELL_WIDTH, DEFAULT_CLIP_DURATION_MS,
};
pub use screenshot_service::{
    BatchScreenshotRequest, BatchScreenshotResult, BatchScreenshotService, ScreenshotFormat,
    ScreenshotManifestEntry, ScreenshotMode, DEFAULT_INTERVAL_SECS, DEFAULT_MAX_SHOTS,
};
pub use video_service::{
    BatchOperationResult, ImportFolderOptions, ImportFolderResult, ImportSkip, VideoReviewService,
};
