//! Worker 可靠性策略：幂等、重试、heartbeat、死信与 artifact 校验。

use serde::{Deserialize, Serialize};

use crate::remote::types::{now_unix, REMOTE_SCHEMA_VERSION};

/// Worker 认领后的租约信息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerLease {
    pub job_id: String,
    pub worker_id: String,
    pub claim_token: String,
    /// 租约过期 Unix 秒；超时未 heartbeat 可被 reclaim。
    pub lease_expires_at: u64,
    pub attempt: u32,
}

/// Heartbeat 请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerHeartbeatRequest {
    pub schema_version: u32,
    pub job_id: String,
    pub worker_id: String,
    pub claim_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed: Option<usize>,
}

impl Default for WorkerHeartbeatRequest {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: String::new(),
            worker_id: String::new(),
            claim_token: String::new(),
            progress: None,
            processed: None,
        }
    }
}

/// 死信记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeadLetterRecord {
    pub schema_version: u32,
    pub job_id: String,
    pub attempts: u32,
    pub last_error: String,
    pub dead_lettered_at: u64,
}

/// Worker / 队列可靠性策略（可序列化到服务端配置）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerReliabilityPolicy {
    /// 最大重试次数（不含首次）。
    pub max_retries: u32,
    /// 租约时长（秒）。
    pub lease_secs: u64,
    /// Heartbeat 间隔建议（秒）。
    pub heartbeat_interval_secs: u64,
    /// 指数退避基数（毫秒）。
    pub backoff_base_ms: u64,
    /// 退避上限（毫秒）。
    pub backoff_max_ms: u64,
    /// 是否要求结果 artifact 带 checksum。
    pub require_artifact_checksum: bool,
}

impl Default for WorkerReliabilityPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            lease_secs: 60,
            heartbeat_interval_secs: 15,
            backoff_base_ms: 500,
            backoff_max_ms: 30_000,
            require_artifact_checksum: true,
        }
    }
}

impl WorkerReliabilityPolicy {
    /// 计算第 `attempt` 次失败后的退避毫秒（attempt 从 1 开始）。
    pub fn backoff_ms(&self, attempt: u32) -> u64 {
        let exp = attempt.saturating_sub(1).min(16);
        let raw = self.backoff_base_ms.saturating_mul(1u64 << exp);
        raw.min(self.backoff_max_ms)
    }

    pub fn should_dead_letter(&self, attempts: u32) -> bool {
        attempts > self.max_retries
    }

    pub fn new_lease(
        &self,
        job_id: impl Into<String>,
        worker_id: impl Into<String>,
        claim_token: impl Into<String>,
        attempt: u32,
    ) -> WorkerLease {
        WorkerLease {
            job_id: job_id.into(),
            worker_id: worker_id.into(),
            claim_token: claim_token.into(),
            lease_expires_at: now_unix().saturating_add(self.lease_secs),
            attempt,
        }
    }
}

/// 幂等键：同一 client_request_id 在同一 workspace 内只创建一次任务。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey {
    pub workspace_id: String,
    pub client_request_id: String,
}

impl IdempotencyKey {
    pub fn new(workspace_id: impl Into<String>, client_request_id: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            client_request_id: client_request_id.into(),
        }
    }
}

/// Artifact 校验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactChecksumStatus {
    Ok,
    Missing,
    Mismatch { expected: String, actual: String },
}

pub fn verify_checksum(expected: Option<&str>, actual: Option<&str>) -> ArtifactChecksumStatus {
    match (expected, actual) {
        (None, _) => ArtifactChecksumStatus::Missing,
        (Some(exp), Some(act)) if exp == act => ArtifactChecksumStatus::Ok,
        (Some(exp), Some(act)) => ArtifactChecksumStatus::Mismatch {
            expected: exp.to_string(),
            actual: act.to_string(),
        },
        (Some(_), None) => ArtifactChecksumStatus::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let p = WorkerReliabilityPolicy::default();
        assert_eq!(p.backoff_ms(1), 500);
        assert_eq!(p.backoff_ms(2), 1000);
        assert_eq!(p.backoff_ms(20), p.backoff_max_ms);
    }

    #[test]
    fn dead_letter_after_max_retries() {
        let p = WorkerReliabilityPolicy::default();
        assert!(!p.should_dead_letter(3));
        assert!(p.should_dead_letter(4));
    }

    #[test]
    fn checksum_mismatch_detected() {
        let s = verify_checksum(Some("abc"), Some("def"));
        assert!(matches!(s, ArtifactChecksumStatus::Mismatch { .. }));
    }
}
