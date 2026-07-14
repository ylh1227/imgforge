//! Redis Streams 任务队列（生产远程栈）。
//!
//! Stream: `imgforge:jobs`
//! Consumer group: `imgforge-workers`
//! DLQ stream: `imgforge:jobs:dlq`

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::{Client, Commands, Connection, RedisResult, Value};

use super::{ClaimedMessage, JobQueue, QueueMessage, QueueResult, WorkerLease};
use crate::server::storage::StoreError;

const STREAM_KEY: &str = "imgforge:jobs";
const DLQ_KEY: &str = "imgforge:jobs:dlq";
const GROUP: &str = "imgforge-workers";

struct InflightMeta {
    stream_id: String,
    job_id: String,
    attempt: u32,
    lease_expires: Instant,
}

pub struct RedisStreamsQueue {
    client: Client,
    /// claim_token -> meta
    inflight: Mutex<HashMap<String, InflightMeta>>,
}

impl RedisStreamsQueue {
    pub fn connect(redis_url: &str) -> Result<Self, String> {
        let client = Client::open(redis_url).map_err(|e| e.to_string())?;
        let mut conn = client.get_connection().map_err(|e| e.to_string())?;
        let _: RedisResult<Value> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(STREAM_KEY)
            .arg(GROUP)
            .arg("0")
            .arg("MKSTREAM")
            .query(&mut conn);
        Ok(Self {
            client,
            inflight: Mutex::new(HashMap::new()),
        })
    }

    fn conn(&self) -> QueueResult<Connection> {
        self.client
            .get_connection()
            .map_err(|e| StoreError::Internal(e.to_string()))
    }

    fn lock(&self) -> QueueResult<MutexGuard<'_, HashMap<String, InflightMeta>>> {
        self.inflight
            .lock()
            .map_err(|_| StoreError::Internal("redis queue lock poisoned".into()))
    }
}

impl JobQueue for RedisStreamsQueue {
    fn enqueue(&self, job_id: &str, attempt: u32) -> QueueResult<()> {
        let mut conn = self.conn()?;
        let _: String = conn
            .xadd(
                STREAM_KEY,
                "*",
                &[("job_id", job_id), ("attempt", &attempt.to_string())],
            )
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn claim(&self, worker_id: &str, lease_secs: u64) -> QueueResult<Option<ClaimedMessage>> {
        let mut conn = self.conn()?;
        let opts = StreamReadOptions::default()
            .group(GROUP, worker_id)
            .count(1)
            .block(200);
        let reply: StreamReadReply = conn
            .xread_options(&[STREAM_KEY], &[">"], &opts)
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        let Some(stream) = reply.keys.first() else {
            return Ok(None);
        };
        let Some(entry) = stream.ids.first() else {
            return Ok(None);
        };

        let mut job_id = String::new();
        let mut attempt = 0u32;
        for (k, v) in &entry.map {
            let val = value_to_string(v);
            if k == "job_id" {
                job_id = val;
            } else if k == "attempt" {
                attempt = val.parse().unwrap_or(0);
            }
        }
        if job_id.is_empty() {
            return Ok(None);
        }

        let claim_token = format!("{}:{}", entry.id, uuid::Uuid::new_v4());
        let message = QueueMessage {
            job_id: job_id.clone(),
            attempt,
            enqueued_at: Instant::now(),
        };
        let lease = WorkerLease {
            job_id: job_id.clone(),
            worker_id: worker_id.to_string(),
            claim_token: claim_token.clone(),
            lease_expires_at: crate::remote::types::now_unix().saturating_add(lease_secs),
            attempt,
        };
        self.lock()?.insert(
            claim_token.clone(),
            InflightMeta {
                stream_id: entry.id.clone(),
                job_id,
                attempt,
                lease_expires: Instant::now() + Duration::from_secs(lease_secs.max(1)),
            },
        );
        Ok(Some(ClaimedMessage { message, lease }))
    }

    fn heartbeat(&self, claim_token: &str, lease_secs: u64) -> QueueResult<()> {
        let mut g = self.lock()?;
        let item = g
            .get_mut(claim_token)
            .ok_or_else(|| StoreError::NotFound(claim_token.into()))?;
        item.lease_expires = Instant::now() + Duration::from_secs(lease_secs.max(1));
        Ok(())
    }

    fn ack(&self, claim_token: &str) -> QueueResult<()> {
        let meta = {
            let mut g = self.lock()?;
            g.remove(claim_token)
                .ok_or_else(|| StoreError::NotFound(claim_token.into()))?
        };
        let mut conn = self.conn()?;
        let _: u64 = conn
            .xack(STREAM_KEY, GROUP, &[&meta.stream_id])
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn reclaim_expired(&self) -> QueueResult<Vec<String>> {
        let now = Instant::now();
        let expired: Vec<InflightMeta> = {
            let mut g = self.lock()?;
            let keys: Vec<String> = g
                .iter()
                .filter(|(_, v)| v.lease_expires <= now)
                .map(|(k, _)| k.clone())
                .collect();
            keys.into_iter().filter_map(|k| g.remove(&k)).collect()
        };
        if expired.is_empty() {
            return Ok(Vec::new());
        }

        let mut conn = self.conn()?;
        let mut job_ids = Vec::new();
        for meta in expired {
            let _: RedisResult<u64> = conn.xack(STREAM_KEY, GROUP, &[&meta.stream_id]);
            let next_attempt = meta.attempt.saturating_add(1);
            let _: RedisResult<String> = conn.xadd(
                STREAM_KEY,
                "*",
                &[
                    ("job_id", meta.job_id.as_str()),
                    ("attempt", &next_attempt.to_string()),
                ],
            );
            job_ids.push(meta.job_id);
        }
        Ok(job_ids)
    }

    fn pending_len(&self) -> QueueResult<usize> {
        let mut conn = self.conn()?;
        let n: i64 = redis::cmd("XLEN")
            .arg(STREAM_KEY)
            .query(&mut conn)
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(n.max(0) as usize)
    }
}

impl RedisStreamsQueue {
    /// 将任务写入死信流（运维可观测）。
    pub fn dead_letter(&self, job_id: &str, reason: &str) -> QueueResult<()> {
        let mut conn = self.conn()?;
        let _: String = conn
            .xadd(DLQ_KEY, "*", &[("job_id", job_id), ("reason", reason)])
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::BulkString(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Value::SimpleString(s) => s.clone(),
        Value::Okay => String::new(),
        Value::Int(n) => n.to_string(),
        Value::Array(items) => items
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(","),
        other => format!("{other:?}"),
    }
}
