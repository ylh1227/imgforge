//! 可靠任务队列抽象（Redis Streams 可替换）。

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::remote::worker_policy::WorkerLease;
use crate::server::storage::StoreError;

pub type QueueResult<T> = Result<T, StoreError>;

/// 入队消息。
#[derive(Debug, Clone)]
pub struct QueueMessage {
    pub job_id: String,
    pub attempt: u32,
    pub enqueued_at: Instant,
}

/// 已认领消息。
#[derive(Debug, Clone)]
pub struct ClaimedMessage {
    pub message: QueueMessage,
    pub lease: WorkerLease,
}

pub trait JobQueue: Send + Sync {
    fn enqueue(&self, job_id: &str, attempt: u32) -> QueueResult<()>;
    fn claim(&self, worker_id: &str, lease_secs: u64) -> QueueResult<Option<ClaimedMessage>>;
    fn heartbeat(&self, claim_token: &str, lease_secs: u64) -> QueueResult<()>;
    fn ack(&self, claim_token: &str) -> QueueResult<()>;
    /// 回收过期租约，返回被 reclaim 的 job_id 列表。
    fn reclaim_expired(&self) -> QueueResult<Vec<String>>;
    fn pending_len(&self) -> QueueResult<usize>;
}

struct Inflight {
    message: QueueMessage,
    #[allow(dead_code)]
    worker_id: String,
    lease_expires: Instant,
}

struct MemoryQueueState {
    pending: VecDeque<QueueMessage>,
    inflight: HashMap<String, Inflight>,
}

pub struct InMemoryQueue {
    inner: Arc<Mutex<MemoryQueueState>>,
}

impl InMemoryQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemoryQueueState {
                pending: VecDeque::new(),
                inflight: HashMap::new(),
            })),
        }
    }

    fn lock(&self) -> QueueResult<std::sync::MutexGuard<'_, MemoryQueueState>> {
        self.inner
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))
    }
}

impl Default for InMemoryQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl JobQueue for InMemoryQueue {
    fn enqueue(&self, job_id: &str, attempt: u32) -> QueueResult<()> {
        let mut g = self.lock()?;
        g.pending.push_back(QueueMessage {
            job_id: job_id.to_string(),
            attempt,
            enqueued_at: Instant::now(),
        });
        Ok(())
    }

    fn claim(&self, worker_id: &str, lease_secs: u64) -> QueueResult<Option<ClaimedMessage>> {
        let mut g = self.lock()?;
        let Some(message) = g.pending.pop_front() else {
            return Ok(None);
        };
        let claim_token = format!(
            "{}:{}:{}",
            worker_id,
            message.job_id,
            message.enqueued_at.elapsed().as_nanos()
        );
        let lease = WorkerLease {
            job_id: message.job_id.clone(),
            worker_id: worker_id.to_string(),
            claim_token: claim_token.clone(),
            lease_expires_at: crate::remote::types::now_unix().saturating_add(lease_secs),
            attempt: message.attempt,
        };
        g.inflight.insert(
            claim_token.clone(),
            Inflight {
                message: message.clone(),
                worker_id: worker_id.to_string(),
                lease_expires: Instant::now() + Duration::from_secs(lease_secs.max(1)),
            },
        );
        Ok(Some(ClaimedMessage { message, lease }))
    }

    fn heartbeat(&self, claim_token: &str, lease_secs: u64) -> QueueResult<()> {
        let mut g = self.lock()?;
        let item = g
            .inflight
            .get_mut(claim_token)
            .ok_or_else(|| StoreError::NotFound(claim_token.into()))?;
        item.lease_expires = Instant::now() + Duration::from_secs(lease_secs.max(1));
        Ok(())
    }

    fn ack(&self, claim_token: &str) -> QueueResult<()> {
        let mut g = self.lock()?;
        g.inflight.remove(claim_token);
        Ok(())
    }

    fn reclaim_expired(&self) -> QueueResult<Vec<String>> {
        let mut g = self.lock()?;
        let now = Instant::now();
        let expired: Vec<String> = g
            .inflight
            .iter()
            .filter(|(_, v)| v.lease_expires <= now)
            .map(|(k, _)| k.clone())
            .collect();
        let mut job_ids = Vec::new();
        for token in expired {
            if let Some(item) = g.inflight.remove(&token) {
                let job_id = item.message.job_id.clone();
                g.pending.push_back(QueueMessage {
                    job_id: job_id.clone(),
                    attempt: item.message.attempt.saturating_add(1),
                    enqueued_at: Instant::now(),
                });
                job_ids.push(job_id);
            }
        }
        Ok(job_ids)
    }

    fn pending_len(&self) -> QueueResult<usize> {
        let g = self.lock()?;
        Ok(g.pending.len())
    }
}

#[cfg(feature = "server")]
pub mod redis_streams;
#[cfg(feature = "server")]
pub use redis_streams::RedisStreamsQueue;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_ack_flow() {
        let q = InMemoryQueue::new();
        q.enqueue("j1", 0).unwrap();
        let claimed = q.claim("w1", 30).unwrap().unwrap();
        assert_eq!(claimed.message.job_id, "j1");
        q.ack(&claimed.lease.claim_token).unwrap();
        assert_eq!(q.pending_len().unwrap(), 0);
    }
}
