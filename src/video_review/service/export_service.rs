//! 视频评审 CSV / JSON / 宫格导出。

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::video_review::domain::{VideoFilter, VideoItem, VideoMarker, VideoSegment, VideoTag};
use crate::video_review::error::VideoReviewResult;
use crate::video_review::service::contact_sheet::{
  ContactSheetRequest, ContactSheetResult, ContactSheetService, FrameProvider,
};
use crate::video_review::service::ffmpeg_backend::ms_to_timestamp;
use crate::video_review::service::grid_video::{
  GridVideoExportRequest, GridVideoExportResult, GridVideoExportService,
};
use crate::video_review::storage::{SqliteVideoRepository, VideoRepository};

#[derive(Debug, Clone)]
pub struct VideoExportRequest {
  pub batch_id: i64,
  pub dest: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoExportResult {
  pub row_count: usize,
  pub dest: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ContactSheetExportRequest {
  pub videos: Vec<VideoItem>,
  pub time_ms: u64,
  pub dest: PathBuf,
}

pub struct VideoExportService;

impl VideoExportService {
  pub fn export_csv(
    repo: &SqliteVideoRepository,
    request: &VideoExportRequest,
  ) -> VideoReviewResult<VideoExportResult> {
    let videos = repo.list_videos(request.batch_id, &VideoFilter::default())?;
    let tags = repo.list_tags()?;
    let tag_map: std::collections::HashMap<i64, &VideoTag> =
      tags.iter().map(|t| (t.id, t)).collect();

    let mut file = File::create(&request.dest)?;
    file.write_all(b"\xEF\xBB\xBF")?;
    let mut wtr = csv::Writer::from_writer(file);
    wtr.write_record([
      "file_path",
      "status",
      "remark",
      "duration",
      "resolution",
      "fps",
      "codec",
      "offset_ms",
      "tags",
      "markers",
      "marker_details",
      "segments",
      "segment_details",
    ])?;

    for video in &videos {
      let tag_ids = repo.get_video_tag_ids(video.id)?;
      let tag_names: Vec<&str> = tag_ids
        .iter()
        .filter_map(|id| tag_map.get(id).map(|t| t.name.as_str()))
        .collect();
      let markers = repo.list_markers(video.id)?;
      let segments = repo.list_segments(video.id)?;
      wtr.write_record([
        video.file_path.to_string_lossy().to_string(),
        video.status.label().to_string(),
        video.remark.clone().unwrap_or_default(),
        video.metadata().duration_label(),
        video.metadata().resolution_label(),
        format!("{:.2}", video.fps),
        video.video_codec.clone(),
        video.offset_ms.to_string(),
        tag_names.join(";"),
        format_markers(&markers),
        format_marker_details(&markers),
        format_segments(&segments),
        format_segment_details(&segments),
      ])?;
    }
    wtr.flush()?;
    Ok(VideoExportResult {
      row_count: videos.len(),
      dest: request.dest.clone(),
    })
  }

  pub fn export_json(repo: &SqliteVideoRepository, batch_id: i64, dest: &Path) -> VideoReviewResult<()> {
    let videos = repo.list_videos(batch_id, &VideoFilter::default())?;
    let tags = repo.list_tags()?;
    let mut payload = Vec::with_capacity(videos.len());
    for video in videos {
      let tag_ids = repo.get_video_tag_ids(video.id)?;
      let tag_names: Vec<String> = tag_ids
        .iter()
        .filter_map(|id| tags.iter().find(|t| t.id == *id).map(|t| t.name.clone()))
        .collect();
      payload.push(VideoJsonRow {
        id: video.id,
        file_path: video.file_path.to_string_lossy().to_string(),
        status: video.status.label().to_string(),
        remark: video.remark.clone(),
        duration_ms: video.duration_ms,
        fps: video.fps,
        width: video.width,
        height: video.height,
        video_codec: video.video_codec.clone(),
        audio_codec: video.audio_codec.clone(),
        offset_ms: video.offset_ms,
        tags: tag_names,
        markers: repo.list_markers(video.id)?,
        segments: repo.list_segments(video.id)?,
      });
    }
    let report = VideoJsonReport {
      exported_at: Utc::now().to_rfc3339(),
      batch_id,
      video_count: payload.len(),
      videos: payload,
      contact_sheets: Vec::new(),
    };
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(dest, json)?;
    Ok(())
  }

  pub fn export_contact_sheet<P: FrameProvider>(
    provider: &P,
    request: &ContactSheetExportRequest,
  ) -> VideoReviewResult<ContactSheetResult> {
    let req = ContactSheetRequest::new(
      request.videos.clone(),
      request.time_ms,
      request.dest.clone(),
    );
    ContactSheetService::export(provider, &req)
  }

  pub fn export_grid_video(request: &GridVideoExportRequest) -> VideoReviewResult<GridVideoExportResult> {
    GridVideoExportService::export("ffmpeg", request)
  }

  pub fn append_contact_sheet_metadata(
    json_path: &Path,
    sheet: &ContactSheetResult,
    time_ms: u64,
    video_ids: &[i64],
  ) -> VideoReviewResult<()> {
    let raw = std::fs::read_to_string(json_path)?;
    let mut report: VideoJsonReport = serde_json::from_str(&raw)?;
    report.contact_sheets.push(ContactSheetMeta {
      exported_at: Utc::now().to_rfc3339(),
      image_path: sheet.dest.to_string_lossy().to_string(),
      width: sheet.width,
      height: sheet.height,
      rows: sheet.rows,
      cols: sheet.cols,
      time_ms,
      video_ids: video_ids.to_vec(),
    });
    std::fs::write(json_path, serde_json::to_string_pretty(&report)?)?;
    Ok(())
  }
}

#[derive(Serialize, Deserialize)]
struct VideoJsonReport {
  exported_at: String,
  batch_id: i64,
  video_count: usize,
  videos: Vec<VideoJsonRow>,
  contact_sheets: Vec<ContactSheetMeta>,
}

#[derive(Serialize, Deserialize)]
struct VideoJsonRow {
  id: i64,
  file_path: String,
  status: String,
  remark: Option<String>,
  duration_ms: u64,
  fps: f32,
  width: u32,
  height: u32,
  video_codec: String,
  audio_codec: Option<String>,
  offset_ms: i64,
  tags: Vec<String>,
  markers: Vec<VideoMarker>,
  segments: Vec<VideoSegment>,
}

#[derive(Serialize, Deserialize)]
pub struct ContactSheetMeta {
  pub exported_at: String,
  pub image_path: String,
  pub width: u32,
  pub height: u32,
  pub rows: usize,
  pub cols: usize,
  pub time_ms: u64,
  pub video_ids: Vec<i64>,
}

fn format_markers(markers: &[VideoMarker]) -> String {
  markers
    .iter()
    .map(|m| format!("{}@{}", m.kind.label(), ms_to_timestamp(m.time_ms)))
    .collect::<Vec<_>>()
    .join("; ")
}

fn format_marker_details(markers: &[VideoMarker]) -> String {
  markers
    .iter()
    .map(|m| {
      format!(
        "{}@{} sev={} {}",
        m.kind.label(),
        ms_to_timestamp(m.time_ms),
        m.severity,
        m.text
      )
    })
    .collect::<Vec<_>>()
    .join("; ")
}

fn format_segments(segments: &[VideoSegment]) -> String {
  segments
    .iter()
    .map(|s| {
      format!(
        "[{}-{}]",
        ms_to_timestamp(s.start_ms),
        ms_to_timestamp(s.end_ms)
      )
    })
    .collect::<Vec<_>>()
    .join("; ")
}

fn format_segment_details(segments: &[VideoSegment]) -> String {
  segments
    .iter()
    .map(|s| {
      format!(
        "[{}-{}] {} ({})",
        ms_to_timestamp(s.start_ms),
        ms_to_timestamp(s.end_ms),
        s.text,
        s.status.label()
      )
    })
    .collect::<Vec<_>>()
    .join("; ")
}
