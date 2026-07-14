//! 从对比视频创建缺陷包（截图 + 对比片段 + 原片 + zip）。

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::review::domain::image_item::ReviewStatus;
use crate::ui::progress::ProgressReporter;
use crate::video_review::domain::{
    DefectManifest, DefectManifestVideo, MarkerKind, VideoDefect, VideoItem,
};
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::export_service::VideoExportService;
use crate::video_review::service::ffmpeg_backend::VideoBackend;
use crate::video_review::service::grid_video::{
    GridVideoCaptionMode, GridVideoExportQuality, GridVideoExportRequest,
};
use crate::video_review::service::video_service::VideoReviewService;
use crate::video_review::storage::VideoRepository;

/// 默认半窗时长（前后各 N 秒）。
pub const DEFAULT_DEFECT_HALF_WINDOW_MS: u64 = 5_000;
/// 对齐置信度低于此值时建议人工确认。
pub const ALIGN_CONFIDENCE_WARN: f32 = 0.35;

#[derive(Debug, Clone)]
pub struct CreateDefectRequest {
    pub batch_id: i64,
    pub title: String,
    pub description: String,
    pub severity: u8,
    pub time_ms: u64,
    pub half_window_ms: u64,
    pub videos: Vec<VideoItem>,
    pub output_dir: PathBuf,
    pub include_grid_png: bool,
    pub include_compare_clip: bool,
    pub include_frames: bool,
    pub include_sources: bool,
    pub quality: GridVideoExportQuality,
    pub mark_issue: bool,
    pub set_needs_fix: bool,
    pub align_method: String,
}

impl Default for CreateDefectRequest {
    fn default() -> Self {
        Self {
            batch_id: 0,
            title: String::new(),
            description: String::new(),
            severity: 2,
            time_ms: 0,
            half_window_ms: DEFAULT_DEFECT_HALF_WINDOW_MS,
            videos: Vec::new(),
            output_dir: PathBuf::new(),
            include_grid_png: true,
            include_compare_clip: true,
            include_frames: true,
            include_sources: true,
            quality: GridVideoExportQuality::Lossless,
            mark_issue: true,
            set_needs_fix: true,
            align_method: "manual".into(),
        }
    }
}

impl CreateDefectRequest {
    /// 轻量包：宫格 + 片段 + 单帧，不含原片。
    pub fn apply_light_preset(&mut self) {
        self.include_grid_png = true;
        self.include_compare_clip = true;
        self.include_frames = true;
        self.include_sources = false;
        self.quality = GridVideoExportQuality::High;
    }

    /// 完整包：含原片 + 无损片段。
    pub fn apply_full_preset(&mut self) {
        self.include_grid_png = true;
        self.include_compare_clip = true;
        self.include_frames = true;
        self.include_sources = true;
        self.quality = GridVideoExportQuality::Lossless;
    }
}

#[derive(Debug, Clone)]
pub struct CreateDefectResult {
    pub defect: VideoDefect,
    pub folder: PathBuf,
    pub zip_path: PathBuf,
}

pub fn create_defect_package(
    service: &VideoReviewService,
    backend: &dyn VideoBackend,
    repo: &dyn VideoRepository,
    req: CreateDefectRequest,
    progress: Option<&dyn ProgressReporter>,
    cancel: Option<&AtomicBool>,
) -> VideoReviewResult<CreateDefectResult> {
    if req.videos.len() < 2 {
        return Err(VideoReviewError::Message(
            "至少选择 2 个对比视频才能新建缺陷".into(),
        ));
    }
    let avail = backend.availability();
    if !avail.ffmpeg_ok {
        return Err(VideoReviewError::FfmpegUnavailable(
            "创建缺陷需要 ffmpeg".into(),
        ));
    }

    let steps = estimate_steps(&req);
    if let Some(p) = progress {
        p.set_total(steps.max(1));
        p.set_current_label("准备目录");
    }

    let stamp = Utc::now().format("%Y%m%d_%H%M%S");
    let slug = sanitize_slug(&req.title);
    let folder_name = if slug.is_empty() {
        format!("defect_{stamp}")
    } else {
        format!("defect_{slug}_{stamp}")
    };
    let folder = req.output_dir.join(&folder_name);
    let zip_path = req.output_dir.join(format!("{folder_name}.zip"));

    let result = (|| -> VideoReviewResult<CreateDefectResult> {
        check_cancel(cancel)?;
        std::fs::create_dir_all(&folder)?;
        std::fs::create_dir_all(folder.join("frames"))?;
        if req.include_sources {
            std::fs::create_dir_all(folder.join("sources"))?;
        }
        step(progress, "准备目录");

        let half = req.half_window_ms.max(500);
        let start_ms = req.time_ms.saturating_sub(half);
        let duration_ms = half.saturating_mul(2).max(500);

        if req.include_grid_png {
            check_cancel(cancel)?;
            if let Some(p) = progress {
                p.set_current_label("导出宫格图");
            }
            let sheet = folder.join("compare_grid.png");
            let _ = service.export_compare_contact_sheet(&req.videos, req.time_ms, sheet)?;
            step(progress, "宫格图");
        }

        if req.include_compare_clip {
            check_cancel(cancel)?;
            if let Some(p) = progress {
                p.set_current_label("导出对比片段");
            }
            let clip_name = if req.quality == GridVideoExportQuality::Lossless {
                "compare_clip_lossless.mp4"
            } else {
                "compare_clip.mp4"
            };
            let clip = folder.join(clip_name);
            let _ = VideoExportService::export_grid_video(&GridVideoExportRequest {
                videos: req.videos.clone(),
                start_time_ms: start_ms,
                duration_ms,
                dest: clip,
                cell_width: 0,
                cell_height: 0,
                quality: req.quality,
                caption_mode: GridVideoCaptionMode::DeviceAndFilename,
            })?;
            step(progress, "对比片段");
        }

        if req.include_frames {
            for video in &req.videos {
                check_cancel(cancel)?;
                if let Some(p) = progress {
                    p.set_current_label(&format!("抽帧 #{}", video.id));
                }
                let t = video.effective_time_ms(req.time_ms).min(video.duration_ms);
                let name = format!(
                    "{}_{}.jpg",
                    video.id,
                    video
                        .file_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("frame")
                );
                let dest = folder.join("frames").join(name);
                backend.extract_frame(&video.file_path, t, 1280, &dest)?;
                step(progress, "单帧");
            }
        }

        if req.include_sources {
            for video in &req.videos {
                check_cancel(cancel)?;
                if let Some(p) = progress {
                    p.set_current_label(&format!("复制原片 #{}", video.id));
                }
                let file_name = video
                    .file_path
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(format!("{}.mp4", video.id)));
                let dest = folder.join("sources").join(file_name);
                if video.file_path != dest {
                    copy_or_hardlink(&video.file_path, &dest).map_err(|e| {
                        VideoReviewError::Message(format!(
                            "复制原片失败 {}: {e}",
                            video.file_path.display()
                        ))
                    })?;
                }
                step(progress, "原片");
            }
        }

        check_cancel(cancel)?;
        if let Some(p) = progress {
            p.set_current_label("写入清单");
        }
        let manifest = DefectManifest {
            title: req.title.clone(),
            description: req.description.clone(),
            severity: req.severity,
            time_ms: req.time_ms,
            half_window_ms: half,
            quality: req.quality.label().to_string(),
            align_method: req.align_method.clone(),
            videos: req
                .videos
                .iter()
                .map(|v| DefectManifestVideo {
                    id: v.id,
                    path: v.file_path.display().to_string(),
                    offset_ms: v.offset_ms,
                    fps: v.fps,
                    effective_time_ms: v.effective_time_ms(req.time_ms),
                    device_model: v.device_model.clone(),
                })
                .collect(),
            created_at_unix: Utc::now().timestamp(),
        };
        std::fs::write(
            folder.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;
        step(progress, "清单");

        if req.mark_issue || req.set_needs_fix {
            check_cancel(cancel)?;
            if let Some(p) = progress {
                p.set_current_label("更新标记/状态");
            }
            for video in &req.videos {
                if req.mark_issue {
                    let _ = repo.add_marker(
                        video.id,
                        video.effective_time_ms(req.time_ms),
                        MarkerKind::Issue,
                        &req.title,
                        req.severity,
                    );
                }
                if req.set_needs_fix {
                    let _ = repo.update_video_status(video.id, ReviewStatus::NeedsFix);
                }
            }
            step(progress, "标记");
        }

        check_cancel(cancel)?;
        if let Some(p) = progress {
            p.set_current_label("压缩 zip");
        }
        zip_directory(&folder, &zip_path)?;
        step(progress, "zip");

        check_cancel(cancel)?;
        if let Some(p) = progress {
            p.set_current_label("写入数据库");
        }
        let defect = repo.create_defect(
            req.batch_id,
            &req.title,
            &req.description,
            req.severity,
            req.time_ms,
            half,
            &req.videos.iter().map(|v| v.id).collect::<Vec<_>>(),
            Some(&zip_path),
        )?;
        step(progress, "数据库");

        if let Some(p) = progress {
            p.finish();
        }

        Ok(CreateDefectResult {
            defect,
            folder: folder.clone(),
            zip_path: zip_path.clone(),
        })
    })();

    if result.is_err() {
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_file(&zip_path);
    }
    result
}

fn check_cancel(cancel: Option<&AtomicBool>) -> VideoReviewResult<()> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(VideoReviewError::Message("已取消打包".into()));
    }
    Ok(())
}

fn estimate_steps(req: &CreateDefectRequest) -> usize {
    let mut n = 1; // prepare
    if req.include_grid_png {
        n += 1;
    }
    if req.include_compare_clip {
        n += 1;
    }
    if req.include_frames {
        n += req.videos.len();
    }
    if req.include_sources {
        n += req.videos.len();
    }
    n += 1; // manifest
    if req.mark_issue || req.set_needs_fix {
        n += 1;
    }
    n += 2; // zip + db
    n
}

fn step(progress: Option<&dyn ProgressReporter>, _label: &str) {
    if let Some(p) = progress {
        p.inc(None);
    }
}

fn copy_or_hardlink(src: &Path, dest: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        if std::fs::hard_link(src, dest).is_ok() {
            return Ok(());
        }
    }
    std::fs::copy(src, dest).map(|_| ())
}

/// 默认输出目录：主视频（第一路）所在文件夹。
pub fn default_defect_output_dir(videos: &[VideoItem]) -> PathBuf {
    videos
        .first()
        .and_then(|v| v.file_path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn sanitize_slug(title: &str) -> String {
    let forbidden = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let s: String = title
        .chars()
        .map(|c| {
            if forbidden.contains(&c) {
                '_'
            } else if c.is_whitespace() {
                '_'
            } else if c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(24)
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    s
}

fn zip_directory(dir: &Path, zip_path: &Path) -> VideoReviewResult<()> {
    let file = File::create(zip_path)?;
    let mut zip = ZipWriter::new(file);
    let mut buffer = Vec::new();
    add_dir_to_zip(&mut zip, dir, dir, &mut buffer)?;
    zip.finish()
        .map_err(|e| VideoReviewError::Message(e.to_string()))?;
    Ok(())
}

fn zip_options_for(rel_path: &str) -> SimpleFileOptions {
    // 原片已压缩，STORE 避免二次膨胀与耗时
    let method = if rel_path.starts_with("sources/") {
        zip::CompressionMethod::Stored
    } else {
        zip::CompressionMethod::Deflated
    };
    SimpleFileOptions::default().compression_method(method)
}

fn add_dir_to_zip(
    zip: &mut ZipWriter<File>,
    root: &Path,
    current: &Path,
    buffer: &mut Vec<u8>,
) -> VideoReviewResult<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let options = zip_options_for(&name);
        if path.is_dir() {
            zip.add_directory(format!("{name}/"), options)
                .map_err(|e| VideoReviewError::Message(e.to_string()))?;
            add_dir_to_zip(zip, root, &path, buffer)?;
        } else {
            zip.start_file(&name, options)
                .map_err(|e| VideoReviewError::Message(e.to_string()))?;
            let mut f = File::open(&path)?;
            buffer.clear();
            f.read_to_end(buffer)?;
            zip.write_all(buffer)?;
        }
    }
    Ok(())
}
