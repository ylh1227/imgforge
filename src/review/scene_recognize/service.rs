//! 批次识别：调 API → 前缀改名 → 标签/备注。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::review::domain::image_item::ImageFilter;
use crate::review::domain::tag::ReviewTag;
use crate::review::error::{ReviewError, ReviewResult};
use crate::review::scene_recognize::catalog::SceneCatalog;
use crate::review::scene_recognize::client::{SceneMatch, VisionSceneClient};
use crate::review::scene_recognize::config::SceneRecognizeConfig;
use crate::review::scene_recognize::naming::apply_scene_prefix;
use crate::review::service::ReviewService;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecognizeItemResult {
    pub image_id: i64,
    pub old_path: String,
    pub new_path: String,
    pub scene_id: Option<String>,
    pub scene_name: Option<String>,
    pub confidence: f32,
    pub renamed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecognizeBatchReport {
    pub batch_id: i64,
    pub total: usize,
    pub matched: usize,
    pub renamed: usize,
    pub failed: usize,
    #[serde(default)]
    pub cancelled: bool,
    pub items: Vec<RecognizeItemResult>,
}

/// 对批次内图片做场景识别并前缀命名。
/// `progress(current, total, message)`：current 从 1 起；开始前会先回调 `(0, total, …)`。
/// `cancel` 为 true 时在**当前项之间**停止（进行中的单次 API 调用仍会跑完）。
pub fn recognize_and_rename_batch(
    service: &ReviewService,
    batch_id: i64,
    progress: Option<&dyn Fn(usize, usize, &str)>,
    cancel: Option<&AtomicBool>,
) -> ReviewResult<RecognizeBatchReport> {
    let catalog = SceneCatalog::load()?;
    catalog.validate()?;
    let config = SceneRecognizeConfig::load()?;
    let client = VisionSceneClient::new(config.clone())?;

    let images = service.list_images(batch_id, &ImageFilter::default())?;
    if images.is_empty() {
        return Err(ReviewError::EmptyBatch);
    }

    let known_names: Vec<String> = catalog.scenes.iter().map(|s| s.name.clone()).collect();
    let name_refs: Vec<&str> = known_names.iter().map(|s| s.as_str()).collect();

    let mut report = RecognizeBatchReport {
        batch_id,
        total: images.len(),
        ..Default::default()
    };

    let palette = ReviewTag::palette();
    let mut tag_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for (i, scene) in catalog.scenes.iter().enumerate() {
        let color = palette[i % palette.len()];
        let id = service.create_tag(&scene.name, color)?;
        tag_ids.insert(scene.name.clone(), id);
    }

    let n = images.len();
    // 两阶段：识别 n + 匹配写回 n，全部完成才到 100%。
    let steps = n.saturating_mul(2).max(1);
    if let Some(cb) = progress {
        cb(0, steps, "准备识别…");
    }

    let workers = config.concurrency.clamp(1, 16);
    let done_count = AtomicUsize::new(0);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .map_err(|e| ReviewError::Message(format!("线程池：{e}")))?;

    // Phase 1: parallel API matches (SQLite writes stay sequential below).
    // Progress uses atomics only — dyn Fn callbacks are !Sync.
    let matches: Vec<(usize, SceneMatch)> = pool.install(|| {
        images
            .par_iter()
            .enumerate()
            .filter_map(|(idx, img)| {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return None;
                }
                let m = client.match_one(img.id, &img.file_path, &catalog);
                done_count.fetch_add(1, Ordering::Relaxed);
                Some((idx, m))
            })
            .collect()
    });

    let finished_api = done_count.load(Ordering::Relaxed).min(n);
    if let Some(cb) = progress {
        cb(
            finished_api,
            steps,
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                "识别阶段已取消"
            } else {
                "识别完成，开始匹配写回…"
            },
        );
    }

    if matches.len() < n && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        report.cancelled = true;
    }

    // Stable order for rename/apply.
    let mut matches = matches;
    matches.sort_by_key(|(idx, _)| *idx);

    // Phase 2: match → rename / tags / remark.
    for (apply_i, (idx, m)) in matches.into_iter().enumerate() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            report.cancelled = true;
            if let Some(cb) = progress {
                cb(n + apply_i, steps, "匹配写回已取消");
            }
            break;
        }

        let img = &images[idx];
        let file_label = img
            .file_path
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("image");
        if let Some(cb) = progress {
            cb(n + apply_i + 1, steps, &format!("匹配写回 {file_label}"));
        }

        let mut item = RecognizeItemResult {
            image_id: img.id,
            old_path: img.file_path.display().to_string(),
            new_path: img.file_path.display().to_string(),
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
            (Some(name), _) => Some(name.clone()),
            (None, true) => Some("未识别".into()),
            (None, false) => None,
        };

        if m.scene_name.is_some() {
            report.matched += 1;
        }

        if let Some(ref prefix_name) = scene_for_prefix {
            match apply_scene_prefix(&img.file_path, Some(prefix_name), &name_refs) {
                Ok(new_path) => {
                    if new_path != img.file_path {
                        if let Err(e) = service.update_image_file_path(img.id, &new_path) {
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
                let _ = service.set_image_tag(img.id, tag_id, true);
            }
            let remark = format!("[场景] {name} | conf={:.2}", m.confidence);
            let _ = service.set_remark(img.id, &remark);
        } else if config.prefix_unknown {
            let _ = service.set_remark(img.id, "[场景] 未识别");
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
