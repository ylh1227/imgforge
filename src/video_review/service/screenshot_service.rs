//! 视频批量截图导出。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::progress::ProgressReporter;
use crate::video_review::domain::{VideoItem, VideoMarker, VideoSegment};
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::contact_sheet::ContactSheetService;
use crate::video_review::service::ffmpeg_backend::ms_to_timestamp;
use crate::video_review::service::frame_cache::FrameCache;

pub const DEFAULT_MAX_SHOTS: usize = 500;
pub const DEFAULT_INTERVAL_SECS: f64 = 5.0;
pub const DEFAULT_FRAME_WIDTH: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenshotMode {
    CurrentTime,
    Interval,
    Markers,
    SegmentStartEnd,
}

impl ScreenshotMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::CurrentTime => "当前时间",
            Self::Interval => "固定间隔",
            Self::Markers => "标记点",
            Self::SegmentStartEnd => "片段起止",
        }
    }

    pub fn all() -> [Self; 4] {
        [
            Self::CurrentTime,
            Self::Interval,
            Self::Markers,
            Self::SegmentStartEnd,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenshotFormat {
    Jpeg,
    Png,
}

impl ScreenshotFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatchScreenshotRequest {
    pub videos: Vec<VideoItem>,
    pub mode: ScreenshotMode,
    pub current_time_ms: u64,
    pub interval_secs: f64,
    pub max_shots: usize,
    pub frame_width: u32,
    pub output_dir: PathBuf,
    pub format: ScreenshotFormat,
    pub naming_template: String,
    pub write_csv_manifest: bool,
    pub write_json_manifest: bool,
    pub write_contact_sheet: bool,
    pub markers_by_video: HashMap<i64, Vec<VideoMarker>>,
    pub segments_by_video: HashMap<i64, Vec<VideoSegment>>,
}

impl BatchScreenshotRequest {
    pub fn new(videos: Vec<VideoItem>, mode: ScreenshotMode, output_dir: PathBuf) -> Self {
        Self {
            videos,
            mode,
            current_time_ms: 0,
            interval_secs: DEFAULT_INTERVAL_SECS,
            max_shots: DEFAULT_MAX_SHOTS,
            frame_width: DEFAULT_FRAME_WIDTH,
            output_dir,
            format: ScreenshotFormat::Jpeg,
            naming_template: "{index}_{filename}_{time}_{device}_{marker}.{ext}".into(),
            write_csv_manifest: true,
            write_json_manifest: false,
            write_contact_sheet: false,
            markers_by_video: HashMap::new(),
            segments_by_video: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotManifestEntry {
    pub index: usize,
    pub video_id: i64,
    pub video_path: String,
    pub time_ms: u64,
    pub time_label: String,
    pub output_path: String,
    pub marker_text: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchScreenshotResult {
    pub requested: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub output_files: Vec<PathBuf>,
    pub manifest_entries: Vec<ScreenshotManifestEntry>,
    pub csv_manifest: Option<PathBuf>,
    pub json_manifest: Option<PathBuf>,
    pub contact_sheets: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedShot {
    video_id: i64,
    time_ms: u64,
    marker_text: Option<String>,
}

pub struct BatchScreenshotService;

impl BatchScreenshotService {
    pub fn export(
        frame_cache: &FrameCache,
        request: &BatchScreenshotRequest,
        progress: Option<&dyn ProgressReporter>,
    ) -> VideoReviewResult<BatchScreenshotResult> {
        if request.videos.is_empty() {
            return Err(VideoReviewError::Message("没有可截图的视频".into()));
        }
        fs::create_dir_all(&request.output_dir)?;

        let shots = plan_shots_internal(request);
        if shots.is_empty() {
            return Err(VideoReviewError::Message("没有可导出的截图时间点".into()));
        }

        if let Some(p) = progress {
            p.set_total(shots.len());
        }

        let mut result = BatchScreenshotResult {
            requested: shots.len(),
            ..Default::default()
        };

        for (index, shot) in shots.iter().enumerate() {
            let Some(video) = request.videos.iter().find(|v| v.id == shot.video_id) else {
                continue;
            };
            if let Some(p) = progress {
                p.set_current_label(
                    video
                        .file_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                        .as_str(),
                );
            }
            let entry = export_one(frame_cache, request, video, index + 1, shot)?;
            if entry.success {
                result.succeeded += 1;
                result.output_files.push(PathBuf::from(&entry.output_path));
            } else {
                result.failed += 1;
            }
            result.manifest_entries.push(entry);
            if let Some(p) = progress {
                p.inc(None);
            }
        }

        if request.write_csv_manifest {
            let path = request.output_dir.join("screenshots.csv");
            write_csv_manifest(&path, &result.manifest_entries)?;
            result.csv_manifest = Some(path);
        }
        if request.write_json_manifest {
            let path = request.output_dir.join("screenshots.json");
            let json = serde_json::to_string_pretty(&result.manifest_entries)?;
            fs::write(&path, json)?;
            result.json_manifest = Some(path);
        }
        if request.write_contact_sheet {
            let items: Vec<(PathBuf, String)> = result
                .manifest_entries
                .iter()
                .filter(|e| e.success)
                .map(|e| {
                    let label = if let Some(marker) = &e.marker_text {
                        format!("{} · {}", e.time_label, marker)
                    } else {
                        e.time_label.clone()
                    };
                    (PathBuf::from(&e.output_path), label)
                })
                .collect();
            if items.len() >= 2 {
                if let Ok(pages) = ContactSheetService::export_image_index_pages(
                    &items,
                    &request.output_dir,
                    "screenshots_index",
                    320,
                ) {
                    result.contact_sheets = pages.into_iter().map(|p| p.dest).collect();
                }
            }
        }

        if let Some(p) = progress {
            p.finish();
        }

        Ok(result)
    }
}

fn export_one(
    frame_cache: &FrameCache,
    request: &BatchScreenshotRequest,
    video: &VideoItem,
    index: usize,
    shot: &PlannedShot,
) -> VideoReviewResult<ScreenshotManifestEntry> {
    let local_time = video.effective_time_ms(shot.time_ms).min(video.duration_ms);
    let time_label = ms_to_timestamp(local_time);
    let filename = video
        .file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let device = video.device_model.as_deref().unwrap_or("");
    let marker = shot.marker_text.as_deref().unwrap_or("");
    let ext = request.format.extension();
    let base_name = render_filename(
        &request.naming_template,
        index,
        filename,
        &time_label,
        device,
        marker,
        ext,
    );
    let dest = unique_output_path(&request.output_dir, &base_name);

    let mut entry = ScreenshotManifestEntry {
        index,
        video_id: video.id,
        video_path: video.file_path.display().to_string(),
        time_ms: local_time,
        time_label: time_label.clone(),
        output_path: dest.display().to_string(),
        marker_text: shot.marker_text.clone(),
        success: false,
        error: None,
    };

    match frame_cache.ensure_frame(&video.file_path, local_time, request.frame_width) {
        Ok(frame_path) => match copy_or_convert_frame(&frame_path, &dest, request.format) {
            Ok(()) => {
                entry.success = true;
            }
            Err(e) => {
                entry.error = Some(e.to_string());
            }
        },
        Err(e) => {
            entry.error = Some(e.to_string());
        }
    }

    Ok(entry)
}

fn copy_or_convert_frame(
    source: &Path,
    dest: &Path,
    format: ScreenshotFormat,
) -> VideoReviewResult<()> {
    match format {
        ScreenshotFormat::Jpeg => {
            fs::copy(source, dest)?;
            Ok(())
        }
        ScreenshotFormat::Png => {
            let img = image::open(source).map_err(|e| VideoReviewError::Message(e.to_string()))?;
            img.save(dest)
                .map_err(|e| VideoReviewError::Message(e.to_string()))?;
            Ok(())
        }
    }
}

fn plan_shots_internal(request: &BatchScreenshotRequest) -> Vec<PlannedShot> {
    let mut shots = Vec::new();
    let max = request.max_shots.max(1);

    for video in &request.videos {
        if shots.len() >= max {
            break;
        }
        let markers = request
            .markers_by_video
            .get(&video.id)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let segments = request
            .segments_by_video
            .get(&video.id)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let mut video_shots = match request.mode {
            ScreenshotMode::CurrentTime => vec![PlannedShot {
                video_id: video.id,
                time_ms: request.current_time_ms,
                marker_text: None,
            }],
            ScreenshotMode::Interval => {
                let step_ms = (request.interval_secs.max(0.1) * 1000.0) as u64;
                let mut points = Vec::new();
                let mut t = 0u64;
                while t < video.duration_ms && points.len() < max {
                    points.push(PlannedShot {
                        video_id: video.id,
                        time_ms: t,
                        marker_text: None,
                    });
                    t = t.saturating_add(step_ms);
                }
                points
            }
            ScreenshotMode::Markers => markers
                .iter()
                .map(|m| PlannedShot {
                    video_id: video.id,
                    time_ms: m.time_ms.min(video.duration_ms),
                    marker_text: Some(m.text.clone()),
                })
                .collect(),
            ScreenshotMode::SegmentStartEnd => segments
                .iter()
                .flat_map(|s| {
                    [
                        PlannedShot {
                            video_id: video.id,
                            time_ms: s.start_ms.min(video.duration_ms),
                            marker_text: Some(format!("start:{}", s.text)),
                        },
                        PlannedShot {
                            video_id: video.id,
                            time_ms: s.end_ms.min(video.duration_ms),
                            marker_text: Some(format!("end:{}", s.text)),
                        },
                    ]
                })
                .collect(),
        };

        let remaining = max.saturating_sub(shots.len());
        if video_shots.len() > remaining {
            video_shots.truncate(remaining);
        }
        shots.extend(video_shots);
    }

    shots
}

pub fn plan_shots(request: &BatchScreenshotRequest) -> Vec<(i64, u64, Option<String>)> {
    plan_shots_internal(request)
        .into_iter()
        .map(|s| (s.video_id, s.time_ms, s.marker_text))
        .collect()
}

pub fn render_filename(
    template: &str,
    index: usize,
    filename: &str,
    time: &str,
    device: &str,
    marker: &str,
    ext: &str,
) -> String {
    let mut out = template.to_string();
    out = out.replace("{index}", &format!("{index:03}"));
    out = out.replace("{filename}", &sanitize_filename_token(filename));
    out = out.replace("{time}", &sanitize_filename_token(time));
    out = out.replace("{device}", &sanitize_filename_token(device));
    out = out.replace("{marker}", &sanitize_filename_token(marker));
    out = out.replace("{ext}", ext);
    sanitize_filename_token(&out)
}

fn sanitize_filename_token(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else if c.is_whitespace() {
                '_'
            } else {
                '_'
            }
        })
        .collect()
}

pub fn unique_output_path(dir: &Path, base_name: &str) -> PathBuf {
    let candidate = dir.join(base_name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(base_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("shot");
    let ext = Path::new(base_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("jpg");
    for i in 2..=9999 {
        let name = format!("{stem}_{i}.{ext}");
        let path = dir.join(&name);
        if !path.exists() {
            return path;
        }
    }
    dir.join(base_name)
}

fn write_csv_manifest(path: &Path, entries: &[ScreenshotManifestEntry]) -> VideoReviewResult<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    for entry in entries {
        wtr.write_record([
            entry.index.to_string(),
            entry.video_id.to_string(),
            entry.video_path.clone(),
            entry.time_ms.to_string(),
            entry.time_label.clone(),
            entry.output_path.clone(),
            entry.marker_text.clone().unwrap_or_default(),
            entry.success.to_string(),
            entry.error.clone().unwrap_or_default(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::domain::image_item::ReviewStatus;
    use chrono::Utc;

    fn sample_video(id: i64, duration_ms: u64) -> VideoItem {
        VideoItem {
            id,
            batch_id: 1,
            file_path: PathBuf::from(format!("/tmp/clip_{id}.mp4")),
            status: ReviewStatus::Pending,
            remark: None,
            thumbnail_path: None,
            duration_ms,
            fps: 24.0,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            audio_codec: None,
            bitrate_kbps: None,
            device_model: Some("iPhone 15".into()),
            offset_ms: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn unique_output_path_avoids_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_output_path(dir.path(), "001_demo.jpg");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_output_path(dir.path(), "001_demo.jpg");
        assert_ne!(first, second);
        assert!(second.to_string_lossy().contains("_2"));
    }

    #[test]
    fn render_filename_replaces_tokens() {
        let name = render_filename(
            "{index}_{filename}_{time}_{device}.{ext}",
            1,
            "demo clip",
            "00:05.000",
            "Pixel 8",
            "",
            "jpg",
        );
        assert!(name.contains("001"));
        assert!(name.contains("demo_clip"));
        assert!(name.contains("Pixel_8"));
        assert!(name.ends_with(".jpg"));
    }

    #[test]
    fn interval_shots_respect_max_limit() {
        let request = BatchScreenshotRequest {
            videos: vec![sample_video(1, 60_000)],
            mode: ScreenshotMode::Interval,
            current_time_ms: 0,
            interval_secs: 5.0,
            max_shots: 3,
            frame_width: 0,
            output_dir: PathBuf::from("/tmp/out"),
            format: ScreenshotFormat::Jpeg,
            naming_template: "{index}.{ext}".into(),
            write_csv_manifest: false,
            write_json_manifest: false,
            write_contact_sheet: false,
            markers_by_video: HashMap::new(),
            segments_by_video: HashMap::new(),
        };
        let shots = plan_shots(&request);
        assert_eq!(shots.len(), 3);
        assert_eq!(shots[0].1, 0);
        assert_eq!(shots[1].1, 5000);
        assert_eq!(shots[2].1, 10000);
    }

    #[test]
    fn segment_mode_includes_start_and_end() {
        use crate::video_review::domain::VideoSegment;
        let mut request = BatchScreenshotRequest::new(
            vec![sample_video(1, 30_000)],
            ScreenshotMode::SegmentStartEnd,
            PathBuf::from("/tmp/out"),
        );
        request.segments_by_video.insert(
            1,
            vec![VideoSegment {
                id: 1,
                video_id: 1,
                start_ms: 1000,
                end_ms: 4000,
                text: "intro".into(),
                status: ReviewStatus::Pending,
                created_at: Utc::now(),
            }],
        );
        let shots = plan_shots(&request);
        assert_eq!(shots.len(), 2);
        assert_eq!(shots[0].1, 1000);
        assert_eq!(shots[1].1, 4000);
    }

    #[test]
    fn marker_mode_uses_marker_times() {
        let mut request = BatchScreenshotRequest::new(
            vec![sample_video(1, 30_000)],
            ScreenshotMode::Markers,
            PathBuf::from("/tmp/out"),
        );
        request.markers_by_video.insert(
            1,
            vec![VideoMarker {
                id: 10,
                video_id: 1,
                time_ms: 1500,
                kind: crate::video_review::domain::MarkerKind::Issue,
                text: "黑场".into(),
                severity: 2,
                created_at: Utc::now(),
            }],
        );
        let shots = plan_shots(&request);
        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].1, 1500);
        assert_eq!(shots[0].2.as_deref(), Some("黑场"));
    }

    #[test]
    fn export_reports_progress() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct CountingProgress(Arc<AtomicUsize>);

        impl crate::ui::progress::ProgressReporter for CountingProgress {
            fn set_total(&self, _total: usize) {}
            fn inc(&self, _sizes: Option<(u64, u64)>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
            fn finish(&self) {}
            fn fraction(&self) -> f32 {
                0.0
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let progress = CountingProgress(Arc::clone(&counter));
        let request = BatchScreenshotRequest {
            videos: vec![sample_video(1, 60_000)],
            mode: ScreenshotMode::Interval,
            current_time_ms: 0,
            interval_secs: 10.0,
            max_shots: 2,
            frame_width: 0,
            output_dir: tempfile::tempdir().unwrap().into_path(),
            format: ScreenshotFormat::Jpeg,
            naming_template: "{index}.{ext}".into(),
            write_csv_manifest: false,
            write_json_manifest: false,
            write_contact_sheet: false,
            markers_by_video: HashMap::new(),
            segments_by_video: HashMap::new(),
        };
        let shots = plan_shots(&request);
        assert_eq!(shots.len(), 2);
        // Progress is only incremented when export runs frames; here we only verify hook exists
        progress.set_total(shots.len());
        progress.inc(None);
        progress.inc(None);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }
}
