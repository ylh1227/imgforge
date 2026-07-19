//! 视频对比缺陷。

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDefect {
    pub id: i64,
    pub batch_id: i64,
    pub title: String,
    pub description: String,
    pub severity: u8,
    pub time_ms: u64,
    pub half_window_ms: u64,
    pub video_ids: Vec<i64>,
    pub package_path: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    /// 已关联的 JIRA Issue Key。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira_issue_key: Option<String>,
    /// JIRA 浏览 URL。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefectManifestVideo {
    pub id: i64,
    pub path: String,
    pub offset_ms: i64,
    pub fps: f32,
    pub effective_time_ms: u64,
    pub device_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefectManifest {
    pub title: String,
    pub description: String,
    pub severity: u8,
    pub time_ms: u64,
    pub half_window_ms: u64,
    pub quality: String,
    pub align_method: String,
    pub videos: Vec<DefectManifestVideo>,
    pub created_at_unix: i64,
}
