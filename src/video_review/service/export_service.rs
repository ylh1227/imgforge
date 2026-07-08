//! 视频评审 CSV / JSON / 宫格导出。

use std::collections::{BTreeMap, HashSet};
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
    pub selected_ids: Option<Vec<i64>>,
    pub schema: VideoExportSchema,
}

impl VideoExportRequest {
    pub fn new(batch_id: i64, dest: PathBuf) -> Self {
        Self {
            batch_id,
            dest,
            selected_ids: None,
            schema: VideoExportSchema::default(),
        }
    }

    pub fn selected(mut self, ids: Vec<i64>) -> Self {
        self.selected_ids = Some(ids);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoExportResult {
    pub row_count: usize,
    pub dest: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoExportColumn {
    pub key: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoExportSchema {
    pub columns: Vec<VideoExportColumn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoExportRow {
    pub video_id: i64,
    pub cells: BTreeMap<String, String>,
}

impl Default for VideoExportSchema {
    fn default() -> Self {
        let columns = [
            ("file_path", "file_path"),
            ("status", "status"),
            ("remark", "remark"),
            ("duration", "duration"),
            ("resolution", "resolution"),
            ("fps", "fps"),
            ("codec", "codec"),
            ("offset_ms", "offset_ms"),
            ("tags", "tags"),
            ("markers", "markers"),
            ("marker_details", "marker_details"),
            ("segments", "segments"),
            ("segment_details", "segment_details"),
        ]
        .into_iter()
        .map(|(key, label)| VideoExportColumn {
            key: key.to_string(),
            label: label.to_string(),
            enabled: true,
        })
        .collect();
        Self { columns }
    }
}

impl VideoExportSchema {
    pub fn with_enabled_keys(mut self, keys: &[String]) -> Self {
        if keys.is_empty() {
            return self;
        }
        let enabled: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for column in &mut self.columns {
            column.enabled = enabled.contains(column.key.as_str());
        }
        self
    }

    pub fn enabled_keys(&self) -> Vec<&str> {
        self.columns
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.key.as_str())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ContactSheetExportRequest {
    pub videos: Vec<VideoItem>,
    pub time_ms: u64,
    pub dest: PathBuf,
}

pub struct VideoExportService;

impl VideoExportService {
    pub fn preview_rows(
        repo: &SqliteVideoRepository,
        request: &VideoExportRequest,
    ) -> VideoReviewResult<Vec<VideoExportRow>> {
        let videos = filtered_videos(repo, request)?;
        let tags = repo.list_tags()?;
        let tag_map: std::collections::HashMap<i64, &VideoTag> =
            tags.iter().map(|t| (t.id, t)).collect();
        videos
            .iter()
            .map(|video| export_row(repo, &tag_map, video))
            .collect()
    }

    pub fn export_csv(
        repo: &SqliteVideoRepository,
        request: &VideoExportRequest,
    ) -> VideoReviewResult<VideoExportResult> {
        let rows = Self::preview_rows(repo, request)?;
        let keys = request.schema.enabled_keys();

        let mut file = File::create(&request.dest)?;
        file.write_all(b"\xEF\xBB\xBF")?;
        let mut wtr = csv::Writer::from_writer(file);
        wtr.write_record(keys.iter().copied())?;

        for row in &rows {
            let record: Vec<String> = keys
                .iter()
                .map(|key| row.cells.get(*key).cloned().unwrap_or_default())
                .collect();
            wtr.write_record(record)?;
        }
        wtr.flush()?;
        Ok(VideoExportResult {
            row_count: rows.len(),
            dest: request.dest.clone(),
        })
    }

    pub fn export_json(
        repo: &SqliteVideoRepository,
        batch_id: i64,
        dest: &Path,
    ) -> VideoReviewResult<()> {
        let request = VideoExportRequest::new(batch_id, dest.to_path_buf());
        Self::export_json_with_request(repo, &request)
    }

    pub fn export_json_with_request(
        repo: &SqliteVideoRepository,
        request: &VideoExportRequest,
    ) -> VideoReviewResult<()> {
        let videos = filtered_videos(repo, request)?;
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
            batch_id: request.batch_id,
            schema: request.schema.clone(),
            rows: Self::preview_rows(repo, request)?,
            video_count: payload.len(),
            videos: payload,
            contact_sheets: Vec::new(),
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(&request.dest, json)?;
        Ok(())
    }

    pub fn export_html_report(
        repo: &SqliteVideoRepository,
        request: &VideoExportRequest,
    ) -> VideoReviewResult<VideoExportResult> {
        let rows = Self::preview_rows(repo, request)?;
        let keys = request.schema.enabled_keys();
        let mut html = String::new();
        html.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><title>ImgForge Video Report</title>");
        html.push_str("<style>body{font-family:-apple-system,BlinkMacSystemFont,sans-serif;margin:24px}table{border-collapse:collapse;width:100%}td,th{border:1px solid #ddd;padding:6px;font-size:12px}th{background:#f4f4f4}.muted{color:#666}</style>");
        html.push_str("</head><body><h1>ImgForge 视频评审报告</h1>");
        html.push_str(&format!("<p class=\"muted\">视频数：{}</p>", rows.len()));
        html.push_str("<table><thead><tr>");
        for key in &keys {
            html.push_str(&format!("<th>{}</th>", escape_html(key)));
        }
        html.push_str("</tr></thead><tbody>");
        for row in &rows {
            html.push_str("<tr>");
            for key in &keys {
                let value = row.cells.get(*key).cloned().unwrap_or_default();
                html.push_str(&format!("<td>{}</td>", escape_html(&value)));
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table><p class=\"muted\">Generated by ImgForge</p></body></html>");
        std::fs::write(&request.dest, html)?;
        Ok(VideoExportResult {
            row_count: rows.len(),
            dest: request.dest.clone(),
        })
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

    pub fn export_grid_video(
        request: &GridVideoExportRequest,
    ) -> VideoReviewResult<GridVideoExportResult> {
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
    schema: VideoExportSchema,
    rows: Vec<VideoExportRow>,
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

fn filtered_videos(
    repo: &SqliteVideoRepository,
    request: &VideoExportRequest,
) -> VideoReviewResult<Vec<VideoItem>> {
    let mut videos = repo.list_videos(request.batch_id, &VideoFilter::default())?;
    if let Some(ids) = &request.selected_ids {
        let wanted: HashSet<i64> = ids.iter().copied().collect();
        videos.retain(|video| wanted.contains(&video.id));
    }
    Ok(videos)
}

fn export_row(
    repo: &SqliteVideoRepository,
    tag_map: &std::collections::HashMap<i64, &VideoTag>,
    video: &VideoItem,
) -> VideoReviewResult<VideoExportRow> {
    let tag_ids = repo.get_video_tag_ids(video.id)?;
    let tag_names: Vec<&str> = tag_ids
        .iter()
        .filter_map(|id| tag_map.get(id).map(|t| t.name.as_str()))
        .collect();
    let markers = repo.list_markers(video.id)?;
    let segments = repo.list_segments(video.id)?;
    let mut cells = BTreeMap::new();
    cells.insert(
        "file_path".into(),
        video.file_path.to_string_lossy().to_string(),
    );
    cells.insert("status".into(), video.status.label().to_string());
    cells.insert("remark".into(), video.remark.clone().unwrap_or_default());
    cells.insert("duration".into(), video.metadata().duration_label());
    cells.insert("resolution".into(), video.metadata().resolution_label());
    cells.insert("fps".into(), format!("{:.2}", video.fps));
    cells.insert("codec".into(), video.video_codec.clone());
    cells.insert("offset_ms".into(), video.offset_ms.to_string());
    cells.insert("tags".into(), tag_names.join(";"));
    cells.insert("markers".into(), format_markers(&markers));
    cells.insert("marker_details".into(), format_marker_details(&markers));
    cells.insert("segments".into(), format_segments(&segments));
    cells.insert("segment_details".into(), format_segment_details(&segments));
    Ok(VideoExportRow {
        video_id: video.id,
        cells,
    })
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

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_review::storage::{NewVideoItem, SqliteVideoRepository, VideoRepository};

    fn sample_item(path: &str) -> NewVideoItem {
        NewVideoItem {
            file_path: PathBuf::from(path),
            thumbnail_path: None,
            duration_ms: 60_000,
            fps: 24.0,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            audio_codec: Some("aac".into()),
            bitrate_kbps: Some(5000),
            device_model: None,
        }
    }

    #[test]
    fn preview_rows_respects_selected_ids() {
        let repo = SqliteVideoRepository::open_memory().unwrap();
        let batch_id = repo
            .create_batch_with_videos(
                "test",
                &[sample_item("/tmp/a.mp4"), sample_item("/tmp/b.mp4")],
            )
            .unwrap();
        let videos = repo.list_videos(batch_id, &VideoFilter::default()).unwrap();
        let request =
            VideoExportRequest::new(batch_id, PathBuf::new()).selected(vec![videos[0].id]);
        let rows = VideoExportService::preview_rows(&repo, &request).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].video_id, videos[0].id);
        assert!(rows[0].cells.contains_key("file_path"));
    }
}
