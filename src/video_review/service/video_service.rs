//! 视频评审业务服务。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jwalk::WalkDir;

use crate::review::domain::image_item::ReviewStatus;
use crate::ui::progress::ProgressReporter;
use crate::video_review::domain::{
    is_video_extension, BatchStats, MarkerKind, VideoBatch, VideoFilter, VideoItem, VideoMarker,
    VideoSegment, VideoTag,
};
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::align_service::{
    AlignBatchResult, AlignService, DEFAULT_ALIGN_SECONDS,
};
use crate::video_review::service::contact_sheet::{ContactSheetResult, FrameProvider};
use crate::video_review::service::defect_package::{
    create_defect_package, CreateDefectRequest, CreateDefectResult,
};
use crate::video_review::service::export_service::{ContactSheetExportRequest, VideoExportService};
use crate::video_review::service::ffmpeg_backend::{
    FfmpegAvailability, FfmpegBackend, VideoBackend,
};
use crate::video_review::service::frame_cache::{FrameCache, FrameCacheStats};
use crate::video_review::service::grid_video::{
    GridVideoCaptionMode, GridVideoExportQuality, GridVideoExportRequest, GridVideoExportResult,
};
use crate::video_review::service::screenshot_service::{
    BatchScreenshotRequest, BatchScreenshotResult, BatchScreenshotService,
};
use crate::video_review::storage::{NewVideoItem, SqliteVideoRepository, VideoRepository};

#[derive(Debug, Clone, Default)]
pub struct ImportFolderOptions {
    /// 导入时是否立刻抽首帧缩略图（关闭可大幅减少 ffmpeg 调用）。
    pub generate_thumbnails: bool,
}

impl ImportFolderOptions {
    pub fn fast() -> Self {
        Self {
            generate_thumbnails: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportSkip {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ImportFolderResult {
    pub batch_id: i64,
    pub imported: usize,
    pub skipped: Vec<ImportSkip>,
}

pub struct VideoReviewService {
    repo: SqliteVideoRepository,
    backend: Arc<dyn VideoBackend>,
    frame_cache: FrameCache,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchOperationResult {
    pub requested: usize,
    pub applied: usize,
    pub failed: usize,
    pub failures: Vec<String>,
}

impl BatchOperationResult {
    fn success(requested: usize) -> Self {
        Self {
            requested,
            applied: requested,
            failed: 0,
            failures: Vec::new(),
        }
    }

    fn failure(requested: usize, error: impl Into<String>) -> Self {
        Self {
            requested,
            applied: 0,
            failed: requested,
            failures: vec![error.into()],
        }
    }

    pub fn is_success(&self) -> bool {
        self.failed == 0
    }
}

impl VideoReviewService {
    pub fn open() -> VideoReviewResult<Self> {
        let backend: Arc<dyn VideoBackend> = Arc::new(FfmpegBackend::with_defaults());
        let frame_cache = FrameCache::new(backend.clone())?;
        Ok(Self {
            repo: SqliteVideoRepository::open()?,
            backend,
            frame_cache,
        })
    }

    pub fn availability(&self) -> FfmpegAvailability {
        self.backend.availability()
    }

    pub fn repo(&self) -> &SqliteVideoRepository {
        &self.repo
    }

    pub fn frame_cache(&self) -> &FrameCache {
        &self.frame_cache
    }

    pub fn list_batches(&self) -> VideoReviewResult<Vec<VideoBatch>> {
        self.repo.list_batches()
    }

    pub fn batch_stats(&self, batch_id: i64) -> VideoReviewResult<BatchStats> {
        self.repo.batch_stats(batch_id)
    }

    pub fn list_videos(
        &self,
        batch_id: i64,
        filter: &VideoFilter,
    ) -> VideoReviewResult<Vec<VideoItem>> {
        self.repo.list_videos(batch_id, filter)
    }

    pub fn get_video(&self, id: i64) -> VideoReviewResult<VideoItem> {
        self.repo.get_video(id)
    }

    pub fn import_folder(
        &self,
        folder: &Path,
        batch_name: Option<&str>,
    ) -> VideoReviewResult<ImportFolderResult> {
        self.import_folder_with_options(
            folder,
            batch_name,
            ImportFolderOptions::fast(),
            None,
        )
    }

    pub fn import_folder_with_options(
        &self,
        folder: &Path,
        batch_name: Option<&str>,
        options: ImportFolderOptions,
        progress: Option<&dyn ProgressReporter>,
    ) -> VideoReviewResult<ImportFolderResult> {
        let name = batch_name
            .map(str::to_string)
            .or_else(|| folder.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "未命名批次".into());

        let paths = collect_video_paths(folder)?;
        if paths.is_empty() {
            return Err(VideoReviewError::Message(
                "文件夹内未找到支持的视频文件".into(),
            ));
        }

        if let Some(p) = progress {
            p.set_total(paths.len().max(1));
            p.set_current_label("探测视频元数据");
        }

        let mut items = Vec::with_capacity(paths.len());
        let mut skipped = Vec::new();
        for path in paths {
            if let Some(p) = progress {
                p.set_current_label(&format!("{}", path.display()));
            }
            let meta = match self.backend.probe_metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("跳过 {}: {}", path.display(), e);
                    skipped.push(ImportSkip {
                        path: path.clone(),
                        reason: e.to_string(),
                    });
                    if let Some(p) = progress {
                        p.inc(None);
                    }
                    continue;
                }
            };
            let thumb = if options.generate_thumbnails {
                self.frame_cache.ensure_frame(&path, 0, 320).ok()
            } else {
                None
            };
            let device_model = meta
                .device_model
                .or_else(|| infer_device_model_from_filename(&path));
            items.push(NewVideoItem {
                file_path: path,
                thumbnail_path: thumb,
                duration_ms: meta.duration_ms,
                fps: meta.fps,
                width: meta.width,
                height: meta.height,
                video_codec: meta.video_codec,
                audio_codec: meta.audio_codec,
                bitrate_kbps: meta.bitrate_kbps,
                device_model,
            });
            if let Some(p) = progress {
                p.inc(None);
            }
        }

        if items.is_empty() {
            let detail = if skipped.is_empty() {
                "未能读取任何视频元数据，请确认已安装 ffprobe".to_string()
            } else {
                let sample: Vec<_> = skipped
                    .iter()
                    .take(3)
                    .map(|s| format!("{} ({})", s.path.display(), s.reason))
                    .collect();
                format!(
                    "未能导入任何视频（跳过 {} 个）。示例：{}",
                    skipped.len(),
                    sample.join("；")
                )
            };
            return Err(VideoReviewError::Message(detail));
        }

        let imported = items.len();
        let batch_id = self.repo.create_batch_with_videos(&name, &items)?;
        if let Some(p) = progress {
            p.finish();
        }
        Ok(ImportFolderResult {
            batch_id,
            imported,
            skipped,
        })
    }

    pub fn update_status(&self, id: i64, status: ReviewStatus) -> VideoReviewResult<()> {
        self.repo.update_video_status(id, status)
    }

    pub fn update_remark(&self, id: i64, remark: &str) -> VideoReviewResult<()> {
        self.repo.update_video_remark(id, remark)
    }

    pub fn update_device_model(
        &self,
        id: i64,
        device_model: Option<&str>,
    ) -> VideoReviewResult<()> {
        self.repo.update_video_device_model(id, device_model)
    }

    pub fn update_offset(&self, id: i64, offset_ms: i64) -> VideoReviewResult<()> {
        self.repo.update_video_offset(id, offset_ms)
    }

    pub fn list_tags(&self) -> VideoReviewResult<Vec<VideoTag>> {
        self.repo.list_tags()
    }

    pub fn create_tag(&self, name: &str, color: [u8; 4]) -> VideoReviewResult<i64> {
        self.repo.create_tag(name, color)
    }

    pub fn set_video_tags(&self, video_id: i64, tag_ids: &[i64]) -> VideoReviewResult<()> {
        self.repo.set_video_tags(video_id, tag_ids)
    }

    pub fn get_video_tag_ids(&self, video_id: i64) -> VideoReviewResult<Vec<i64>> {
        self.repo.get_video_tag_ids(video_id)
    }

    pub fn add_marker(
        &self,
        video_id: i64,
        time_ms: u64,
        kind: MarkerKind,
        text: &str,
        severity: u8,
    ) -> VideoReviewResult<i64> {
        self.repo
            .add_marker(video_id, time_ms, kind, text, severity)
    }

    pub fn list_markers(&self, video_id: i64) -> VideoReviewResult<Vec<VideoMarker>> {
        self.repo.list_markers(video_id)
    }

    pub fn delete_marker(&self, id: i64) -> VideoReviewResult<()> {
        self.repo.delete_marker(id)
    }

    pub fn add_segment(
        &self,
        video_id: i64,
        start_ms: u64,
        end_ms: u64,
        text: &str,
        status: ReviewStatus,
    ) -> VideoReviewResult<i64> {
        self.repo
            .add_segment(video_id, start_ms, end_ms, text, status)
    }

    pub fn list_segments(&self, video_id: i64) -> VideoReviewResult<Vec<VideoSegment>> {
        self.repo.list_segments(video_id)
    }

    pub fn delete_segment(&self, id: i64) -> VideoReviewResult<()> {
        self.repo.delete_segment(id)
    }

    pub fn frame_at(
        &self,
        video: &VideoItem,
        global_time_ms: u64,
        width: u32,
    ) -> VideoReviewResult<Option<PathBuf>> {
        let t = video
            .effective_time_ms(global_time_ms)
            .min(video.duration_ms);
        self.frame_cache.get_or_request(&video.file_path, t, width)
    }

    pub fn ensure_cover(&self, video: &VideoItem) -> VideoReviewResult<PathBuf> {
        self.frame_cache.ensure_frame(&video.file_path, 0, 480)
    }

    pub fn timeline_thumbs(
        &self,
        video: &VideoItem,
        count: usize,
    ) -> VideoReviewResult<Vec<(u64, Option<PathBuf>)>> {
        let count = count.clamp(4, 24);
        if video.duration_ms == 0 {
            return Ok(Vec::new());
        }
        let step = video.duration_ms / count as u64;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let t = (step * i as u64).min(video.duration_ms.saturating_sub(1));
            let path = self.frame_at(video, t, 160).ok().flatten();
            out.push((t, path));
        }
        Ok(out)
    }

    pub fn ensure_frame_sync(
        &self,
        video: &VideoItem,
        global_time_ms: u64,
        width: u32,
    ) -> VideoReviewResult<PathBuf> {
        let t = video
            .effective_time_ms(global_time_ms)
            .min(video.duration_ms);
        self.frame_cache.ensure_frame(&video.file_path, t, width)
    }

    pub fn export_compare_contact_sheet(
        &self,
        videos: &[VideoItem],
        time_ms: u64,
        dest: PathBuf,
    ) -> VideoReviewResult<ContactSheetResult> {
        VideoExportService::export_contact_sheet(
            self,
            &ContactSheetExportRequest {
                videos: videos.to_vec(),
                time_ms,
                dest,
            },
        )
    }

    pub fn export_compare_grid_video(
        &self,
        videos: &[VideoItem],
        start_time_ms: u64,
        duration_ms: u64,
        dest: PathBuf,
        quality: GridVideoExportQuality,
        caption_mode: GridVideoCaptionMode,
    ) -> VideoReviewResult<GridVideoExportResult> {
        VideoExportService::export_grid_video(&GridVideoExportRequest {
            videos: videos.to_vec(),
            start_time_ms,
            duration_ms,
            dest,
            cell_width: 0,
            cell_height: 0,
            quality,
            caption_mode,
        })
    }

    /// 音频互相关对齐到主视频（第一路）。`around_ms` 为当前对比时间，用于截取分析窗。
    pub fn align_videos(
        &self,
        reference: &VideoItem,
        others: &[VideoItem],
        around_ms: Option<u64>,
    ) -> VideoReviewResult<AlignBatchResult> {
        AlignService::new(self.backend.ffmpeg_bin()).align_to_reference(
            reference,
            others,
            DEFAULT_ALIGN_SECONDS,
            around_ms,
        )
    }

    /// 从当前对比视频打包缺陷（目录 + zip + DB 记录）。
    pub fn create_defect(
        &self,
        req: CreateDefectRequest,
        progress: Option<&dyn ProgressReporter>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> VideoReviewResult<CreateDefectResult> {
        create_defect_package(
            self,
            self.backend.as_ref(),
            &self.repo,
            req,
            progress,
            cancel,
        )
    }

    pub fn list_defects(&self, batch_id: i64) -> VideoReviewResult<Vec<crate::video_review::domain::VideoDefect>> {
        self.repo.list_defects(batch_id)
    }

    pub fn update_defect_jira(
        &self,
        defect_id: i64,
        issue_key: &str,
        browse_url: Option<&str>,
    ) -> VideoReviewResult<()> {
        self.repo
            .update_defect_jira(defect_id, issue_key, browse_url)
    }

    pub fn batch_update_status(&self, ids: &[i64], status: ReviewStatus) -> VideoReviewResult<()> {
        self.repo.batch_update_status(ids, status)
    }

    pub fn batch_update_status_result(
        &self,
        ids: &[i64],
        status: ReviewStatus,
    ) -> BatchOperationResult {
        match self.repo.batch_update_status(ids, status) {
            Ok(()) => BatchOperationResult::success(ids.len()),
            Err(e) => BatchOperationResult::failure(ids.len(), e.to_string()),
        }
    }

    pub fn batch_append_remark(&self, ids: &[i64], text: &str) -> VideoReviewResult<()> {
        self.repo.batch_append_remark(ids, text)
    }

    pub fn batch_append_remark_result(&self, ids: &[i64], text: &str) -> BatchOperationResult {
        match self.repo.batch_append_remark(ids, text) {
            Ok(()) => BatchOperationResult::success(ids.len()),
            Err(e) => BatchOperationResult::failure(ids.len(), e.to_string()),
        }
    }

    pub fn batch_set_tags(&self, ids: &[i64], tag_ids: &[i64]) -> VideoReviewResult<()> {
        self.repo.batch_set_tags(ids, tag_ids)
    }

    pub fn batch_set_tags_result(&self, ids: &[i64], tag_ids: &[i64]) -> BatchOperationResult {
        match self.repo.batch_set_tags(ids, tag_ids) {
            Ok(()) => BatchOperationResult::success(ids.len()),
            Err(e) => BatchOperationResult::failure(ids.len(), e.to_string()),
        }
    }

    pub fn frame_cache_stats(&self) -> VideoReviewResult<FrameCacheStats> {
        self.frame_cache.stats()
    }

    pub fn clear_frame_cache(&self) -> VideoReviewResult<usize> {
        self.frame_cache.clear()
    }

    pub fn export_batch_screenshots(
        &self,
        request: &BatchScreenshotRequest,
        progress: Option<&dyn ProgressReporter>,
    ) -> VideoReviewResult<BatchScreenshotResult> {
        BatchScreenshotService::export(self.frame_cache(), request, progress)
    }

    pub fn markers_for_videos(
        &self,
        video_ids: &[i64],
    ) -> VideoReviewResult<std::collections::HashMap<i64, Vec<VideoMarker>>> {
        let mut out = std::collections::HashMap::new();
        for id in video_ids {
            out.insert(*id, self.list_markers(*id)?);
        }
        Ok(out)
    }

    pub fn segments_for_videos(
        &self,
        video_ids: &[i64],
    ) -> VideoReviewResult<std::collections::HashMap<i64, Vec<VideoSegment>>> {
        let mut out = std::collections::HashMap::new();
        for id in video_ids {
            out.insert(*id, self.list_segments(*id)?);
        }
        Ok(out)
    }
}

impl FrameProvider for VideoReviewService {
    fn ensure_frame(
        &self,
        video: &VideoItem,
        global_time_ms: u64,
        width: u32,
    ) -> VideoReviewResult<PathBuf> {
        self.ensure_frame_sync(video, global_time_ms, width)
    }
}

fn collect_video_paths(folder: &Path) -> VideoReviewResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(folder) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(is_video_extension)
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn infer_device_model_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy();
    let normalized = stem.replace(['-', '_', '.'], " ");
    for token in normalized.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        if lower.starts_with("iphone") && token.len() > "iphone".len() {
            return Some(format_iphone_token(token));
        }
        if lower.starts_with("pixel") && token.len() > "pixel".len() {
            return Some(format!("Pixel {}", &token["Pixel".len()..]));
        }
        if lower.starts_with("galaxy") && token.len() > "galaxy".len() {
            return Some(format!("Galaxy {}", &token["Galaxy".len()..]));
        }
    }
    None
}

fn format_iphone_token(token: &str) -> String {
    if let Some(raw) = token.get("iPhone".len()..) {
        format!("iPhone {raw}")
    } else {
        "iPhone".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn collect_video_paths_filters() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.mp4"), b"").unwrap();
        fs::write(dir.path().join("b.txt"), b"").unwrap();
        fs::write(dir.path().join("c.MOV"), b"").unwrap();
        let paths = collect_video_paths(dir.path()).unwrap();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn infers_device_model_from_common_filenames() {
        assert_eq!(
            infer_device_model_from_filename(Path::new("/tmp/iPhone15_clip.mov")).as_deref(),
            Some("iPhone 15")
        );
        assert_eq!(
            infer_device_model_from_filename(Path::new("/tmp/Pixel8_sample.mp4")).as_deref(),
            Some("Pixel 8")
        );
    }
}
