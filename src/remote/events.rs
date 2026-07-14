//! 状态同步升级：SSE / 事件流契约（轮询仍为兼容路径）。

use serde::{Deserialize, Serialize};

use crate::remote::types::{RemoteJobPhase, RemoteJobSource, REMOTE_SCHEMA_VERSION};

/// 任务事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteJobEventKind {
    Queued,
    Running,
    Progress,
    Succeeded,
    Failed,
    Cancelled,
    Heartbeat,
    Log,
}

impl From<RemoteJobPhase> for RemoteJobEventKind {
    fn from(phase: RemoteJobPhase) -> Self {
        match phase {
            RemoteJobPhase::Queued => Self::Queued,
            RemoteJobPhase::Running => Self::Running,
            RemoteJobPhase::Succeeded => Self::Succeeded,
            RemoteJobPhase::Failed => Self::Failed,
            RemoteJobPhase::Cancelled => Self::Cancelled,
            RemoteJobPhase::Unknown => Self::Log,
        }
    }
}

/// 单条任务事件（SSE `data:` JSON 或 gRPC stream 消息）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteJobEvent {
    pub schema_version: u32,
    pub job_id: String,
    pub source: RemoteJobSource,
    pub kind: RemoteJobEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<RemoteJobPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub ts: u64,
}

impl RemoteJobEvent {
    pub fn phase_change(
        job_id: impl Into<String>,
        source: RemoteJobSource,
        phase: RemoteJobPhase,
        ts: u64,
    ) -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: job_id.into(),
            source,
            kind: RemoteJobEventKind::from(phase),
            phase: Some(phase),
            progress: None,
            processed: None,
            total: None,
            message: None,
            ts,
        }
    }
}

/// 客户端订阅偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteStatusTransport {
    /// 兼容路径：轮询 `GET /v1/jobs`。
    #[default]
    Poll,
    /// `GET /v1/jobs/{id}/events` Server-Sent Events。
    Sse,
    /// 后续：tonic gRPC bidirectional / server streaming。
    GrpcStream,
}

/// SSE 端点约定说明（文档化常量）。
pub const JOB_EVENTS_PATH_TEMPLATE: &str = "/v1/jobs/{id}/events";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::types::RemoteJobPhase;

    #[test]
    fn phase_maps_to_event_kind() {
        let e = RemoteJobEvent::phase_change(
            "j1",
            RemoteJobSource::Convert,
            RemoteJobPhase::Running,
            1,
        );
        assert_eq!(e.kind, RemoteJobEventKind::Running);
    }
}
