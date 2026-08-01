//! 视频批次场景识别：抽代表帧 → 云端匹配 → 文件名前缀。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::review::scene_recognize::{
    apply_scene_prefix, SceneCatalog, SceneMatch, SceneRecognizeConfig, VisionSceneClient,
};
use crate::video_review::domain::{VideoFilter, VideoItem, VideoTag};
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::VideoReviewService;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoRecognizeItemResult {
    pub video_id: i64,
    pub old_path: String,
    pub new_path: String,
    pub frame_path: Option<String>,
    pub scene_id: Option<String>,
    pub scene_name: Option<String>,
    pub confidence: f32,
    pub renamed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoRecognizeBatchReport {
    pub batch_id: i64,
    pub total: usize,
    pub matched: usize,
    pub renamed: usize,
    pub failed: usize,
    #[serde(default)]
    pub cancelled: bool,
    pub items: Vec<VideoRecognizeItemResult>,
}

struct PreparedVideo {
    idx: usize,
    video_id: i64,
    file_path: PathBuf,
    sample_ms: u64,
    frame_path: Option<PathBuf>,
    frame_error: Option<String>,
}

/// 对视频批次抽帧识别场景，并按场景名给视频文件加前缀。
/// `progress(current, total, message)`：current 从 1 起；开始前会先回调 `(0, total, …)`。
/// `cancel` 为 true 时在**当前项之间**停止（进行中的抽帧/API 仍会跑完）。
pub fn recognize_and_rename_video_batch(
    service: &VideoReviewService,
    batch_id: i64,
    progress: Option<&dyn Fn(usize, usize, &str)>,
    cancel: Option<&AtomicBool>,
) -> VideoReviewResult<VideoRecognizeBatchReport> {
    let catalog = SceneCatalog::load().map_err(|e| VideoReviewError::Message(e.to_string()))?;
    catalog
        .validate()
        .map_err(|e| VideoReviewError::Message(e.to_string()))?;
    let config =
        SceneRecognizeConfig::load().map_err(|e| VideoReviewError::Message(e.to_string()))?;
    let client =
        VisionSceneClient::new(config.clone()).map_err(|e| VideoReviewError::Message(e.to_string()))?;

    let videos = service.list_videos(batch_id, &VideoFilter::default())?;
    if videos.is_empty() {
        return Err(VideoReviewError::Message("批次内没有视频".into()));
    }

    let known_names: Vec<String> = catalog.scenes.iter().map(|s| s.name.clone()).collect();
    let name_refs: Vec<&str> = known_names.iter().map(|s| s.as_str()).collect();

    let mut report = VideoRecognizeBatchReport {
        batch_id,
        total: videos.len(),
        ..Default::default()
    };

    let palette = VideoTag::PALETTE;
    let mut tag_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for (i, scene) in catalog.scenes.iter().enumerate() {
        let color = palette[i % palette.len()];
        let id = service.create_tag(&scene.name, color)?;
        tag_ids.insert(scene.name.clone(), id);
    }

    let frame_width = config.max_edge.clamp(256, 1280);
    let n = videos.len();
    // 三阶段：抽帧 n + 识别 n + 匹配写回 n，全部完成才到 100%。
    let steps = n.saturating_mul(3).max(1);
    if let Some(cb) = progress {
        cb(0, steps, "准备抽帧识别…");
    }

    // Phase 1: sequential frame extract (service/SQLite is !Sync).
    let mut prepared: Vec<PreparedVideo> = Vec::with_capacity(videos.len());
    for (idx, video) in videos.iter().enumerate() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            report.cancelled = true;
            if let Some(cb) = progress {
                cb(idx, steps, "抽帧已取消");
            }
            break;
        }
        let sample_ms = sample_pts_ms(video);
        let file_label = video
            .file_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("video");
        if let Some(cb) = progress {
            cb(idx + 1, steps, &format!("抽帧 {file_label}"));
        }
        let (frame_path, frame_error) =
            match service.ensure_frame_sync(video, sample_ms, frame_width) {
                Ok(p) => (Some(p), None),
                Err(e) => (None, Some(format!("抽帧失败：{e}"))),
            };
        prepared.push(PreparedVideo {
            idx,
            video_id: video.id,
            file_path: video.file_path.clone(),
            sample_ms,
            frame_path,
            frame_error,
        });
    }

    if report.cancelled {
        return Ok(report);
    }

    let workers = config.concurrency.clamp(1, 16);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .map_err(|e| VideoReviewError::Message(format!("线程池：{e}")))?;

    let done_count = AtomicUsize::new(0);
    let api_total = prepared.len();

    // Phase 2: parallel vision API.
    let matched: Vec<(PreparedVideo, SceneMatch)> = pool.install(|| {
        prepared
            .into_par_iter()
            .filter_map(|prep| {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return None;
                }
                let m = if let Some(err) = &prep.frame_error {
                    SceneMatch {
                        image_id: prep.video_id,
                        scene_id: None,
                        scene_name: None,
                        confidence: 0.0,
                        error: Some(err.clone()),
                    }
                } else if let Some(frame) = &prep.frame_path {
                    client.match_one(prep.video_id, frame, &catalog)
                } else {
                    SceneMatch {
                        image_id: prep.video_id,
                        scene_id: None,
                        scene_name: None,
                        confidence: 0.0,
                        error: Some("抽帧失败".into()),
                    }
                };
                done_count.fetch_add(1, Ordering::Relaxed);
                Some((prep, m))
            })
            .collect()
    });

    let finished_api = done_count.load(Ordering::Relaxed).min(n);
    if let Some(cb) = progress {
        cb(
            n + finished_api,
            steps,
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                "识别阶段已取消"
            } else {
                "识别完成，开始匹配写回…"
            },
        );
    }

    if matched.len() < api_total && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        report.cancelled = true;
    }

    let mut matched = matched;
    matched.sort_by_key(|(p, _)| p.idx);

    // Phase 3: match → rename / tags / remark.
    for (apply_i, (prep, m)) in matched.into_iter().enumerate() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            report.cancelled = true;
            if let Some(cb) = progress {
                cb(2 * n + apply_i, steps, "匹配写回已取消");
            }
            break;
        }

        let file_label = prep
            .file_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("video");
        if let Some(cb) = progress {
            cb(
                2 * n + apply_i + 1,
                steps,
                &format!("匹配写回 {file_label}"),
            );
        }

        let mut item = VideoRecognizeItemResult {
            video_id: prep.video_id,
            old_path: prep.file_path.display().to_string(),
            new_path: prep.file_path.display().to_string(),
            frame_path: prep.frame_path.as_ref().map(|p| p.display().to_string()),
            scene_id: m.scene_id.clone(),
            scene_name: m.scene_name.clone(),
            confidence: m.confidence,
            renamed: false,
            error: m.error.clone(),
        };

        if item.error.is_some() {
            report.failed += 1;
            report.items.push(item);
            continue;
        }

        let scene_for_prefix: Option<String> = match (&m.scene_name, config.prefix_unknown) {
            (Some(n), _) => Some(n.clone()),
            (None, true) => Some("未识别".into()),
            (None, false) => None,
        };

        if m.scene_name.is_some() {
            report.matched += 1;
        }

        if let Some(ref prefix_name) = scene_for_prefix {
            match apply_scene_prefix(&prep.file_path, Some(prefix_name), &name_refs) {
                Ok(new_path) => {
                    if new_path != prep.file_path {
                        if let Err(e) = service.update_file_path(prep.video_id, &new_path) {
                            item.error = Some(e.to_string());
                            report.failed += 1;
                            report.items.push(item);
                            continue;
                        }
                        item.new_path = new_path.display().to_string();
                        item.renamed = true;
                        report.renamed += 1;
                    }
                }
                Err(e) => {
                    item.error = Some(format!("重命名失败：{e}"));
                    report.failed += 1;
                    report.items.push(item);
                    continue;
                }
            }
        }

        if let Some(ref name) = m.scene_name {
            if let Some(&tag_id) = tag_ids.get(name) {
                let mut ids = service.get_video_tag_ids(prep.video_id).unwrap_or_default();
                if !ids.contains(&tag_id) {
                    ids.push(tag_id);
                }
                let _ = service.set_video_tags(prep.video_id, &ids);
            }
            let remark = format!(
                "[场景] {name} | conf={:.2} | t={}ms",
                m.confidence, prep.sample_ms
            );
            let _ = service.update_remark(prep.video_id, &remark);
        } else if config.prefix_unknown {
            let _ = service.update_remark(prep.video_id, "[场景] 未识别");
        }

        report.items.push(item);
    }

    if !report.cancelled {
        if let Some(cb) = progress {
            cb(steps, steps, "识别与匹配全部完成");
        }
    }

    Ok(report)
}

fn sample_pts_ms(video: &VideoItem) -> u64 {
    if video.duration_ms > 2_000 {
        (video.duration_ms / 10).min(video.duration_ms.saturating_sub(1))
    } else {
        0
    }
}
