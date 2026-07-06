//! 视频评审服务层。

pub mod contact_sheet;
pub mod export_service;
pub mod ffmpeg_backend;
pub mod frame_cache;
pub mod grid_video;
pub mod video_service;

pub use contact_sheet::{
  compute_layout, grid_dimensions, ContactSheetRequest, ContactSheetResult, ContactSheetService,
  FrameProvider, GridLayout,
};
pub use grid_video::{
  compute_quality_cell_size, max_export_duration_ms, GridVideoExportQuality,
  GridVideoExportRequest, GridVideoExportResult, GridVideoExportService,
  DEFAULT_CLIP_DURATION_MS, DEFAULT_CELL_HEIGHT, DEFAULT_CELL_WIDTH,
};
pub use export_service::{
  ContactSheetExportRequest, ContactSheetMeta, VideoExportRequest, VideoExportResult,
  VideoExportService,
};
pub use ffmpeg_backend::{
  parse_ffprobe_json, parse_frame_rate, FfmpegAvailability, FfmpegBackend, FfmpegConfig,
  VideoBackend,
};
pub use frame_cache::FrameCache;
pub use video_service::VideoReviewService;
