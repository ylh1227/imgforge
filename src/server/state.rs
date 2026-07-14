//! 共享应用状态：优先远程栈（Postgres / Redis / S3），未配置时回退开发后备。

use std::sync::Arc;

use crate::remote::worker_policy::WorkerReliabilityPolicy;
use crate::server::config::ServerConfig;
use crate::server::object_store::{DiskObjectStore, MemoryObjectStore, ObjectStore, S3ObjectStore};
use crate::server::queue::{InMemoryQueue, JobQueue, RedisStreamsQueue};
use crate::server::rate_limit::RateLimiter;
use crate::server::storage::{
    InMemoryJobStore, JobStore, PostgresJobStore, SqliteJobStore, StoreError,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub store: Arc<dyn JobStore>,
    pub queue: Arc<dyn JobQueue>,
    pub objects: Arc<dyn ObjectStore>,
    pub policy: WorkerReliabilityPolicy,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub fn in_memory(config: ServerConfig) -> Self {
        let policy = config.worker.clone();
        let rate_limiter = Arc::new(RateLimiter::new(config.rate_limit_per_minute));
        Self {
            config: Arc::new(config),
            store: Arc::new(InMemoryJobStore::new()),
            queue: Arc::new(InMemoryQueue::new()),
            objects: Arc::new(MemoryObjectStore::new()),
            policy,
            rate_limiter,
        }
    }

    /// 按配置组装远程优先栈；缺省组件回退到 SQLite / 内存队列 / 磁盘对象存储。
    pub fn from_config(config: ServerConfig) -> Result<Self, StoreError> {
        config
            .ensure_dirs()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        let policy = config.worker.clone();
        let rate_limiter = Arc::new(RateLimiter::new(config.rate_limit_per_minute));

        let store: Arc<dyn JobStore> = if config.uses_postgres() {
            let url = config.database_url.as_ref().unwrap();
            Arc::new(PostgresJobStore::connect(url)?)
        } else {
            Arc::new(SqliteJobStore::open(config.sqlite_path())?)
        };

        let queue: Arc<dyn JobQueue> = if config.uses_redis() {
            let url = config.redis_url.as_ref().unwrap();
            Arc::new(
                RedisStreamsQueue::connect(url)
                    .map_err(|e| StoreError::Internal(format!("redis: {e}")))?,
            )
        } else {
            Arc::new(InMemoryQueue::new())
        };

        let objects: Arc<dyn ObjectStore> = if config.uses_s3() {
            Arc::new(
                S3ObjectStore::from_config(&config.s3, config.object_store_public_base())
                    .map_err(|e| StoreError::Internal(format!("s3: {e}")))?,
            )
        } else {
            Arc::new(DiskObjectStore::new(
                config.objects_dir(),
                config.public_base.clone(),
            )?)
        };

        for job_id in store.list_queued_job_ids()? {
            let _ = queue.enqueue(&job_id, 0);
        }

        Ok(Self {
            config: Arc::new(config),
            store,
            queue,
            objects,
            policy,
            rate_limiter,
        })
    }

    /// 兼容旧名：开发后备（SQLite + 磁盘 + 内存队列）。
    pub fn local_disk(config: ServerConfig) -> Result<Self, StoreError> {
        Self::from_config(config)
    }
}
