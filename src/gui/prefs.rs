//! GUI 用户偏好：转换预设与任务历史（`~/.imgforge/gui_prefs.json`）。

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::types::{BrightnessMatchMode, ImageFormat, Quality, ResizeOptions};

const MAX_HISTORY: usize = 12;
const MAX_ACTION_HISTORY: usize = 20;
const MAX_EXPORT_TEMPLATES: usize = 20;
const MAX_REVIEW_COMMENTS: usize = 200;
const MAX_PRESETS: usize = 20;

fn prefs_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("imgforge").join("gui_prefs.json")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".imgforge").join("gui_prefs.json"))
            .unwrap_or_else(|_| PathBuf::from("gui_prefs.json"))
    }
}

/// 可保存/恢复的转换参数快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertPresetSnapshot {
    pub format: ImageFormat,
    pub quality: u8,
    pub resize: ResizeOptions,
    pub recursive: bool,
    pub preserve_structure: bool,
    pub overwrite: bool,
    pub strip_metadata: bool,
    pub bayer_only: bool,
    pub rename_template: String,
    pub target_max_bytes: Option<u64>,
    pub use_target_max_bytes: bool,
    #[serde(default)]
    pub brightness_match_enabled: bool,
    #[serde(default)]
    pub brightness_match_mode: BrightnessMatchMode,
    #[serde(default)]
    pub brightness_match_path: String,
    #[serde(default = "default_bm_metric_percentile")]
    pub brightness_match_metric_percentile: bool,
    #[serde(default = "default_bm_percentile")]
    pub brightness_match_percentile: f32,
    #[serde(default)]
    pub brightness_match_regional: bool,
}

fn default_bm_metric_percentile() -> bool {
    true
}

fn default_bm_percentile() -> f32 {
    98.0
}

impl Default for ConvertPresetSnapshot {
    fn default() -> Self {
        Self {
            format: ImageFormat::WebP,
            quality: Quality::DEFAULT.value(),
            resize: ResizeOptions {
                width: None,
                height: None,
                mode: crate::core::types::ResizeMode::Fit,
            },
            recursive: true,
            preserve_structure: true,
            overwrite: false,
            strip_metadata: false,
            bayer_only: false,
            rename_template: String::new(),
            target_max_bytes: None,
            use_target_max_bytes: false,
            brightness_match_enabled: false,
            brightness_match_mode: BrightnessMatchMode::Paired,
            brightness_match_path: String::new(),
            brightness_match_metric_percentile: default_bm_metric_percentile(),
            brightness_match_percentile: default_bm_percentile(),
            brightness_match_regional: false,
        }
    }
}

/// 最近一次亮度匹配偏好（应用启动恢复）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrightnessMatchPrefs {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: BrightnessMatchMode,
    #[serde(default)]
    pub path: String,
    #[serde(default = "default_bm_metric_percentile")]
    pub metric_percentile: bool,
    #[serde(default = "default_bm_percentile")]
    pub percentile: f32,
    #[serde(default)]
    pub regional: bool,
}

impl Default for BrightnessMatchPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: BrightnessMatchMode::Paired,
            path: String::new(),
            metric_percentile: default_bm_metric_percentile(),
            percentile: default_bm_percentile(),
            regional: false,
        }
    }
}

impl BrightnessMatchPrefs {
    pub fn is_empty(&self) -> bool {
        !self.enabled
            && self.mode == BrightnessMatchMode::Paired
            && self.path.is_empty()
            && self.metric_percentile
            && (self.percentile - 98.0).abs() < f32::EPSILON
            && !self.regional
    }
}

/// 单次转换任务历史记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryEntry {
    pub finished_at_unix: u64,
    pub input_dir: String,
    pub output_dir: String,
    pub successes: usize,
    pub failures: usize,
    pub total: usize,
    pub elapsed_ms: u64,
    pub snapshot: ConvertPresetSnapshot,
}

/// 跨模块的用户可感知操作历史记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionHistoryEntry {
    pub finished_at_unix: u64,
    pub module: String,
    pub operation: String,
    pub target: String,
    pub status: ActionHistoryStatus,
    pub success_count: usize,
    pub failure_count: usize,
    pub total_count: usize,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionHistoryStatus {
    Succeeded,
    PartiallyFailed,
    Failed,
}

impl ActionHistoryStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "完成",
            Self::PartiallyFailed => "部分失败",
            Self::Failed => "失败",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportTemplate {
    pub module: String,
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub created_at_unix: u64,
    pub author: String,
    pub target: String,
    pub body: String,
    #[serde(default)]
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomReviewStatus {
    pub key: String,
    pub label: String,
    pub color_rgba: [u8; 4],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuiPrefs {
    pub presets: Vec<NamedPreset>,
    #[serde(default)]
    pub history: Vec<TaskHistoryEntry>,
    #[serde(default)]
    pub action_history: Vec<ActionHistoryEntry>,
    #[serde(default)]
    pub export_templates: Vec<ExportTemplate>,
    #[serde(default = "default_author_name")]
    pub reviewer_name: String,
    #[serde(default)]
    pub review_comments: Vec<ReviewComment>,
    #[serde(default)]
    pub custom_statuses: Vec<CustomReviewStatus>,
    /// JIRA 非 secret 偏好（token 永不写入）。
    #[serde(
        default,
        skip_serializing_if = "crate::jira::JiraPrefsSnapshot::is_empty"
    )]
    pub jira: crate::jira::JiraPrefsSnapshot,
    /// 最近亮度匹配偏好。
    #[serde(default, skip_serializing_if = "BrightnessMatchPrefs::is_empty")]
    pub brightness_match: BrightnessMatchPrefs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedPreset {
    pub name: String,
    pub snapshot: ConvertPresetSnapshot,
}

impl GuiPrefs {
    pub fn load() -> Self {
        let path = prefs_path();
        if !path.exists() {
            return Self::default();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = prefs_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        fs::write(path, json)
    }

    pub fn upsert_preset(&mut self, name: String, snapshot: ConvertPresetSnapshot) {
        if let Some(p) = self.presets.iter_mut().find(|p| p.name == name) {
            p.snapshot = snapshot;
        } else {
            self.presets.push(NamedPreset { name, snapshot });
            if self.presets.len() > MAX_PRESETS {
                self.presets.remove(0);
            }
        }
    }

    pub fn delete_preset(&mut self, name: &str) {
        self.presets.retain(|p| p.name != name);
    }

    pub fn push_history(&mut self, entry: TaskHistoryEntry) {
        self.history.insert(0, entry);
        self.history.truncate(MAX_HISTORY);
    }

    pub fn push_action_history(&mut self, entry: ActionHistoryEntry) {
        self.action_history.insert(0, entry);
        self.action_history.truncate(MAX_ACTION_HISTORY);
    }

    pub fn upsert_export_template(&mut self, template: ExportTemplate) {
        if let Some(existing) = self
            .export_templates
            .iter_mut()
            .find(|t| t.module == template.module && t.name == template.name)
        {
            *existing = template;
        } else {
            self.export_templates.insert(0, template);
            self.export_templates.truncate(MAX_EXPORT_TEMPLATES);
        }
    }

    pub fn export_templates_for(&self, module: &str) -> Vec<ExportTemplate> {
        self.export_templates
            .iter()
            .filter(|template| template.module == module)
            .cloned()
            .collect()
    }

    pub fn push_review_comment(&mut self, comment: ReviewComment) {
        self.review_comments.insert(0, comment);
        self.review_comments.truncate(MAX_REVIEW_COMMENTS);
    }
}

fn default_author_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Reviewer".into())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_history_is_capped_newest_first() {
        let mut prefs = GuiPrefs::default();
        for i in 0..25 {
            prefs.push_action_history(ActionHistoryEntry {
                finished_at_unix: i,
                module: "测试".into(),
                operation: format!("op-{i}"),
                target: "target".into(),
                status: ActionHistoryStatus::Succeeded,
                success_count: 1,
                failure_count: 0,
                total_count: 1,
                elapsed_ms: 0,
                detail: None,
            });
        }
        assert_eq!(prefs.action_history.len(), MAX_ACTION_HISTORY);
        assert_eq!(prefs.action_history[0].operation, "op-24");
    }

    #[test]
    fn export_template_upsert_replaces_same_name() {
        let mut prefs = GuiPrefs::default();
        prefs.upsert_export_template(ExportTemplate {
            module: "数据提取".into(),
            name: "默认".into(),
            columns: vec!["a".into()],
        });
        prefs.upsert_export_template(ExportTemplate {
            module: "数据提取".into(),
            name: "默认".into(),
            columns: vec!["b".into()],
        });
        let templates = prefs.export_templates_for("数据提取");
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].columns, vec!["b"]);
    }

    #[test]
    fn review_comments_are_capped() {
        let mut prefs = GuiPrefs::default();
        for i in 0..205 {
            prefs.push_review_comment(ReviewComment {
                created_at_unix: i,
                author: "a".into(),
                target: "t".into(),
                body: format!("c{i}"),
                resolved: false,
            });
        }
        assert_eq!(prefs.review_comments.len(), MAX_REVIEW_COMMENTS);
        assert_eq!(prefs.review_comments[0].body, "c204");
    }

    #[test]
    fn convert_preset_snapshot_missing_brightness_defaults() {
        let json = r#"{
            "format": "webp",
            "quality": 85,
            "resize": {"width": null, "height": null, "mode": "fit"},
            "recursive": true,
            "preserve_structure": true,
            "overwrite": false,
            "strip_metadata": false,
            "bayer_only": false,
            "rename_template": "",
            "target_max_bytes": null,
            "use_target_max_bytes": false
        }"#;
        let snap: ConvertPresetSnapshot = serde_json::from_str(json).unwrap();
        assert!(!snap.brightness_match_enabled);
        assert_eq!(snap.brightness_match_mode, BrightnessMatchMode::Paired);
        assert!(snap.brightness_match_path.is_empty());
        assert!(snap.brightness_match_metric_percentile);
        assert!((snap.brightness_match_percentile - 98.0).abs() < f32::EPSILON);
        assert!(!snap.brightness_match_regional);
    }
}
