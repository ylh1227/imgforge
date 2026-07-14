//! 远端任务离线缓存：任务摘要、最近同步时间、产物元数据。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::remote::error::{RemoteError, RemoteResult};
use crate::remote::types::{now_unix, RemoteAssetRef, RemoteJobSummary};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteJobCache {
    pub schema_version: u32,
    #[serde(default)]
    pub last_sync_at: Option<u64>,
    #[serde(default)]
    pub last_health_ok: Option<bool>,
    #[serde(default)]
    pub last_health_message: Option<String>,
    #[serde(default)]
    pub jobs: Vec<RemoteJobSummary>,
    #[serde(default)]
    pub artifacts: Vec<CachedArtifact>,
}

impl Default for RemoteJobCache {
    fn default() -> Self {
        Self {
            schema_version: crate::remote::types::REMOTE_SCHEMA_VERSION,
            last_sync_at: None,
            last_health_ok: None,
            last_health_message: None,
            jobs: Vec::new(),
            artifacts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedArtifact {
    pub job_id: String,
    pub asset: RemoteAssetRef,
}

impl RemoteJobCache {
    pub fn load(path: &Path) -> RemoteResult<Self> {
        if !path.exists() {
            return Ok(Self {
                schema_version: crate::remote::types::REMOTE_SCHEMA_VERSION,
                ..Self::default()
            });
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|e| RemoteError::Cache(format!("read {}: {e}", path.display())))?;
        toml::from_str(&raw).map_err(|e| RemoteError::Cache(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> RemoteResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RemoteError::Cache(format!("mkdir {}: {e}", parent.display())))?;
        }
        let raw = toml::to_string_pretty(self).map_err(|e| RemoteError::Cache(e.to_string()))?;
        std::fs::write(path, raw)
            .map_err(|e| RemoteError::Cache(format!("write {}: {e}", path.display())))?;
        Ok(())
    }

    pub fn upsert_jobs(&mut self, jobs: Vec<RemoteJobSummary>) {
        self.jobs = jobs;
        self.last_sync_at = Some(now_unix());
    }

    pub fn record_health(&mut self, ok: bool, message: impl Into<String>) {
        self.last_health_ok = Some(ok);
        self.last_health_message = Some(message.into());
    }

    pub fn replace_artifacts_for_job(&mut self, job_id: &str, assets: Vec<RemoteAssetRef>) {
        self.artifacts.retain(|a| a.job_id != job_id);
        self.artifacts
            .extend(assets.into_iter().map(|asset| CachedArtifact {
                job_id: job_id.to_string(),
                asset,
            }));
    }
}

/// 缓存读写门面。
#[derive(Debug, Clone)]
pub struct RemoteCacheStore {
    path: PathBuf,
}

impl RemoteCacheStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> RemoteResult<RemoteJobCache> {
        RemoteJobCache::load(&self.path)
    }

    pub fn save(&self, cache: &RemoteJobCache) -> RemoteResult<()> {
        cache.save(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::types::{RemoteJobPhase, RemoteJobSource};

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote_jobs.toml");
        let store = RemoteCacheStore::new(path.clone());
        let mut cache = store.load().unwrap();
        cache.upsert_jobs(vec![RemoteJobSummary {
            job_id: "j1".into(),
            source: RemoteJobSource::Convert,
            phase: RemoteJobPhase::Running,
            progress: Some(0.5),
            processed: 1,
            total: 2,
            updated_at: 123,
            error_summary: None,
        }]);
        cache.record_health(true, "ok");
        store.save(&cache).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.jobs.len(), 1);
        assert_eq!(loaded.jobs[0].job_id, "j1");
        assert_eq!(loaded.last_health_ok, Some(true));
        assert!(path.exists());
    }
}
