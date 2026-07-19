//! 本地评审/缺陷 → JIRA 字段映射与描述模板。

use std::path::Path;

use serde_json::{json, Value};

use crate::jira::config::{JiraApiVersion, JiraConfig};

const SUMMARY_MAX: usize = 240;
const REMARK_SUMMARY_MAX: usize = 80;
const MAX_ANNOTATION_SUMMARY: usize = 10;

#[derive(Debug, Clone)]
pub struct MappedIssue {
    pub summary: String,
    pub description_text: String,
    pub priority: String,
    pub labels: Vec<String>,
}

#[cfg(feature = "review")]
pub fn map_review_item(
    cfg: &JiraConfig,
    item: &crate::review::domain::image_item::ReviewImageItem,
) -> MappedIssue {
    map_review_item_with_annotations(cfg, item, None)
}

#[cfg(feature = "review")]
pub fn map_review_item_with_annotations(
    cfg: &JiraConfig,
    item: &crate::review::domain::image_item::ReviewImageItem,
    annotation_summary: Option<&str>,
) -> MappedIssue {
    let filename = item
        .file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    // 英文可检索前缀（to_sql）；中文状态放描述。
    let mut summary = format!("[{}] {}", item.status.to_sql(), filename);
    if !item.remark.trim().is_empty() {
        let remark = truncate(item.remark.trim(), REMARK_SUMMARY_MAX);
        summary = format!("{summary} — {remark}");
    }
    summary = truncate(&summary, SUMMARY_MAX);

    let mut labels = cfg.labels.clone();
    labels.push(status_label_key(item.status).to_string());
    labels.sort();
    labels.dedup();

    let mut description_text = format!(
        "来源: ImgForge 图片评审\n\
         文件: {}\n\
         状态: {} ({})\n\
         备注: {}\n\
         标注数: {}\n\
         尺寸: {}\n\
         批次 ID: {}\n\
         条目 ID: {}",
        item.file_path.display(),
        item.status.label(),
        item.status.to_sql(),
        if item.remark.is_empty() {
            "（无）"
        } else {
            item.remark.as_str()
        },
        item.annotation_count,
        format_dims(item.width, item.height),
        item.batch_id,
        item.id,
    );
    if let Some(summary) = annotation_summary.filter(|s| !s.trim().is_empty()) {
        description_text.push_str("\n\n标注摘要:\n");
        description_text.push_str(summary.trim());
    }

    MappedIssue {
        summary,
        description_text,
        priority: cfg.default_priority.clone(),
        labels,
    }
}

#[cfg(feature = "review")]
pub fn format_annotation_summary(
    annotations: &[crate::review::domain::annotation::Annotation],
) -> String {
    use crate::review::domain::annotation::{AnnotationKind, AnnotationPosition};

    let mut lines = Vec::new();
    for ann in annotations.iter().take(MAX_ANNOTATION_SUMMARY) {
        let line = match (&ann.kind, &ann.position) {
            (AnnotationKind::Rectangle, AnnotationPosition::Rectangle(r)) => {
                format!(
                    "- rect ({:.2},{:.2})-({:.2},{:.2})",
                    r.x0, r.y0, r.x1, r.y1
                )
            }
            (AnnotationKind::Arrow, AnnotationPosition::Arrow(a)) => {
                format!(
                    "- arrow ({:.2},{:.2})→({:.2},{:.2})",
                    a.x0, a.y0, a.x1, a.y1
                )
            }
            (AnnotationKind::Text, AnnotationPosition::Text(t)) => {
                let content = if ann.content.trim().is_empty() {
                    "（空）"
                } else {
                    ann.content.trim()
                };
                format!("- text @({:.2},{:.2}): \"{}\"", t.x, t.y, truncate(content, 60))
            }
            _ => format!("- {:?}", ann.kind),
        };
        lines.push(line);
    }
    if annotations.len() > MAX_ANNOTATION_SUMMARY {
        lines.push(format!(
            "- …另有 {} 条",
            annotations.len() - MAX_ANNOTATION_SUMMARY
        ));
    }
    lines.join("\n")
}

#[cfg(feature = "video-review")]
pub fn map_video_defect(
    cfg: &JiraConfig,
    defect: &crate::video_review::domain::VideoDefect,
) -> MappedIssue {
    map_video_defect_with_manifest(cfg, defect, None)
}

#[cfg(feature = "video-review")]
pub fn map_video_defect_with_manifest(
    cfg: &JiraConfig,
    defect: &crate::video_review::domain::VideoDefect,
    manifest_summary: Option<&str>,
) -> MappedIssue {
    let summary = if defect.title.trim().is_empty() {
        format!("视频缺陷 #{}", defect.id)
    } else {
        truncate(defect.title.trim(), SUMMARY_MAX)
    };

    let mut labels = cfg.labels.clone();
    labels.push("video-defect".into());
    labels.push(format!("severity-{}", defect.severity));
    labels.sort();
    labels.dedup();

    let package = defect
        .package_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "（无）".into());

    let mut description_text = format!(
        "来源: ImgForge 视频缺陷包\n\
         标题: {}\n\
         描述: {}\n\
         严重度: {}\n\
         时间点: {} ms（±{} ms）\n\
         关联视频 ID: {:?}\n\
         缺陷包: {}\n\
         批次 ID: {}\n\
         缺陷 ID: {}",
        defect.title,
        if defect.description.is_empty() {
            "（无）"
        } else {
            defect.description.as_str()
        },
        defect.severity,
        defect.time_ms,
        defect.half_window_ms,
        defect.video_ids,
        package,
        defect.batch_id,
        defect.id,
    );
    if let Some(extra) = manifest_summary.filter(|s| !s.trim().is_empty()) {
        description_text.push_str("\n\n缺陷包清单:\n");
        description_text.push_str(extra.trim());
    }

    MappedIssue {
        summary,
        description_text,
        priority: cfg.priority_for_severity(defect.severity),
        labels,
    }
}

/// 从缺陷 zip 旁的 `manifest.json`（或 zip 同目录展开）读取摘要；失败返回 None。
#[cfg(feature = "video-review")]
pub fn try_load_defect_manifest_summary(package_path: Option<&Path>) -> Option<String> {
    let package = package_path?;
    let candidates = [
        package
            .parent()
            .map(|p| p.join("manifest.json"))
            .unwrap_or_else(|| Path::new("manifest.json").to_path_buf()),
        package.with_extension("").join("manifest.json"),
    ];
    for path in candidates {
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&path).ok()?;
        let manifest: crate::video_review::domain::DefectManifest =
            serde_json::from_str(&raw).ok()?;
        let mut lines = vec![
            format!("- align: {}", manifest.align_method),
            format!("- quality: {}", manifest.quality),
            format!("- videos: {}", manifest.videos.len()),
        ];
        for v in manifest.videos.iter().take(8) {
            let device = v
                .device_model
                .as_deref()
                .unwrap_or("-");
            lines.push(format!(
                "  · id={} offset={}ms fps={:.1} device={} path={}",
                v.id, v.offset_ms, v.fps, device, v.path
            ));
        }
        if manifest.videos.len() > 8 {
            lines.push(format!("  · …另有 {} 路", manifest.videos.len() - 8));
        }
        return Some(lines.join("\n"));
    }
    None
}

/// 将纯文本描述转为 create-issue 可用的 description 字段值。
pub fn description_field(api: JiraApiVersion, text: &str) -> Value {
    match api {
        JiraApiVersion::V3 => adf_doc(text),
        JiraApiVersion::V2 => Value::String(text.to_string()),
    }
}

pub fn build_create_fields(cfg: &JiraConfig, mapped: &MappedIssue) -> Value {
    let project_key = cfg
        .project_key
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let mut fields = json!({
        "project": { "key": project_key },
        "summary": mapped.summary,
        "description": description_field(cfg.api_version, &mapped.description_text),
        "issuetype": { "name": cfg.issue_type },
        "labels": mapped.labels,
    });
    if !mapped.priority.trim().is_empty() {
        fields["priority"] = json!({ "name": mapped.priority });
    }
    if let Some(obj) = fields.as_object_mut() {
        for (k, v) in &cfg.extra_fields {
            obj.insert(k.clone(), v.clone());
        }
    }
    fields
}

fn adf_doc(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .lines()
        .map(|line| {
            json!({
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": if line.is_empty() { " " } else { line }
                }]
            })
        })
        .collect();
    json!({
        "type": "doc",
        "version": 1,
        "content": if paragraphs.is_empty() {
            vec![json!({
                "type": "paragraph",
                "content": [{ "type": "text", "text": " " }]
            })]
        } else {
            paragraphs
        }
    })
}

#[cfg(feature = "review")]
fn status_label_key(status: crate::review::domain::image_item::ReviewStatus) -> &'static str {
    use crate::review::domain::image_item::ReviewStatus;
    match status {
        ReviewStatus::Pending => "pending",
        ReviewStatus::Approved => "approved",
        ReviewStatus::NeedsFix => "needs-fix",
        ReviewStatus::Rejected => "rejected",
    }
}

#[cfg(feature = "review")]
fn format_dims(w: Option<u32>, h: Option<u32>) -> String {
    match (w, h) {
        (Some(w), Some(h)) => format!("{w}×{h}"),
        _ => "未知".into(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn attachment_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("attachment.bin")
        .to_string()
}

#[cfg(all(test, feature = "review"))]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    use crate::review::domain::image_item::{ReviewImageItem, ReviewStatus};

    fn sample_item() -> ReviewImageItem {
        ReviewImageItem {
            id: 7,
            batch_id: 3,
            file_path: PathBuf::from("/tmp/shot.jpg"),
            status: ReviewStatus::NeedsFix,
            remark: "曝光偏高".into(),
            thumbnail_path: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
            file_size: Some(1024),
            width: Some(1920),
            height: Some(1080),
            convert_params: Default::default(),
            annotation_count: 2,
            jira_issue_key: None,
            jira_url: None,
        }
    }

    #[test]
    fn maps_review_summary_english_prefix() {
        let cfg = JiraConfig {
            project_key: Some("CAM".into()),
            ..JiraConfig::default()
        };
        let mapped = map_review_item(&cfg, &sample_item());
        assert!(mapped.summary.contains("[need_fix]"));
        assert!(mapped.summary.contains("shot.jpg"));
        assert!(mapped.description_text.contains("待修正"));
        let fields = build_create_fields(&cfg, &mapped);
        assert_eq!(fields["project"]["key"], "CAM");
        assert_eq!(fields["description"]["type"], "doc");
    }

    #[test]
    fn annotation_summary_appended() {
        let cfg = JiraConfig::default();
        let mapped = map_review_item_with_annotations(
            &cfg,
            &sample_item(),
            Some("- rect (0.10,0.20)-(0.30,0.40)"),
        );
        assert!(mapped.description_text.contains("标注摘要"));
        assert!(mapped.description_text.contains("rect"));
    }

    #[test]
    fn v2_description_is_string() {
        let cfg = JiraConfig {
            api_version: crate::jira::config::JiraApiVersion::V2,
            project_key: Some("CAM".into()),
            ..JiraConfig::default()
        };
        let mapped = map_review_item(&cfg, &sample_item());
        let fields = build_create_fields(&cfg, &mapped);
        assert!(fields["description"].is_string());
    }
}
