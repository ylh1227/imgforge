//! 远端任务 / 素材 / 结果数据契约（schema 可演进）。

use serde::{Deserialize, Serialize};

/// 当前客户端理解的远端 API schema 版本。
pub const REMOTE_SCHEMA_VERSION: u32 = 1;

/// 远端素材引用：不绑定本地路径，便于后续从服务器加载。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAssetRef {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
}

/// 任务来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteJobSource {
    #[default]
    Convert,
    Review,
    VideoReview,
    DataExtract,
    Other,
}

impl RemoteJobSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Convert => "格式转换",
            Self::Review => "图片评审",
            Self::VideoReview => "视频评审",
            Self::DataExtract => "数据提取",
            Self::Other => "其他",
        }
    }
}

/// 远端任务生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteJobPhase {
    #[default]
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

impl RemoteJobPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "排队中",
            Self::Running => "运行中",
            Self::Succeeded => "已完成",
            Self::Failed => "失败",
            Self::Cancelled => "已取消",
            Self::Unknown => "未知",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// 提交到服务器的任务请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteJobRequest {
    pub schema_version: u32,
    pub source: RemoteJobSource,
    /// 工作区 / 租户标识（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// 输入素材（远端引用或待上传占位）。
    #[serde(default)]
    pub inputs: Vec<RemoteAssetRef>,
    /// 任务参数（格式、质量等），以稳定键值表达，避免绑定本地路径。
    #[serde(default)]
    pub params: RemoteJobParams,
    /// 客户端侧幂等键，便于重试去重。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
}

impl Default for RemoteJobRequest {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            source: RemoteJobSource::Convert,
            workspace_id: None,
            inputs: Vec::new(),
            params: RemoteJobParams::default(),
            client_request_id: None,
        }
    }
}

/// 转换类任务参数（首期聚焦批处理）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RemoteJobParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recursive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve_structure: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bayer_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_max_bytes: Option<u64>,
    /// 预留扩展字段（字符串化 JSON / 键值），避免 schema 频繁破坏。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<(String, String)>,
}

/// 远端任务状态快照。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteJobStatus {
    pub schema_version: u32,
    pub job_id: String,
    pub source: RemoteJobSource,
    pub phase: RemoteJobPhase,
    /// 0.0–1.0；未知时为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(default)]
    pub processed: usize,
    #[serde(default)]
    pub total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_summary: Option<String>,
    /// Unix 秒。
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
}

impl RemoteJobStatus {
    pub fn new(job_id: impl Into<String>, source: RemoteJobSource, phase: RemoteJobPhase) -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: job_id.into(),
            source,
            phase,
            progress: None,
            processed: 0,
            total: 0,
            error_summary: None,
            log_summary: None,
            updated_at: now_unix(),
            created_at: Some(now_unix()),
        }
    }
}

/// 任务完成后的结果与产物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteJobResult {
    pub schema_version: u32,
    pub job_id: String,
    pub phase: RemoteJobPhase,
    #[serde(default)]
    pub successes: usize,
    #[serde(default)]
    pub failures: usize,
    #[serde(default)]
    pub artifacts: Vec<RemoteAssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
    pub updated_at: u64,
}

/// 任务列表摘要（任务中心展示）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteJobSummary {
    pub job_id: String,
    pub source: RemoteJobSource,
    pub phase: RemoteJobPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(default)]
    pub processed: usize,
    #[serde(default)]
    pub total: usize,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
}

impl From<&RemoteJobStatus> for RemoteJobSummary {
    fn from(status: &RemoteJobStatus) -> Self {
        Self {
            job_id: status.job_id.clone(),
            source: status.source,
            phase: status.phase,
            progress: status.progress,
            processed: status.processed,
            total: status.total,
            updated_at: status.updated_at,
            error_summary: status.error_summary.clone(),
        }
    }
}

/// 远端连接健康状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHealth {
    pub ok: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_request_defaults_to_schema_v1() {
        let req = RemoteJobRequest::default();
        assert_eq!(req.schema_version, REMOTE_SCHEMA_VERSION);
        assert_eq!(req.source, RemoteJobSource::Convert);
    }

    #[test]
    fn phase_terminal_detection() {
        assert!(RemoteJobPhase::Succeeded.is_terminal());
        assert!(!RemoteJobPhase::Running.is_terminal());
    }

    #[test]
    fn summary_from_status() {
        let status =
            RemoteJobStatus::new("job-1", RemoteJobSource::Convert, RemoteJobPhase::Queued);
        let summary = RemoteJobSummary::from(&status);
        assert_eq!(summary.job_id, "job-1");
        assert_eq!(summary.phase, RemoteJobPhase::Queued);
    }
}
