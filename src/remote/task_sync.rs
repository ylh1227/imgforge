//! 任务同步服务：上传输入、提交远端任务、等待完成并下载产物。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::config::AppConfig;
use crate::io::scanner::{ScanFilter, ScanOptions};
use crate::remote::cache::{RemoteCacheStore, RemoteJobCache};
use crate::remote::client::RemoteClient;
use crate::remote::config::RemoteConfig;
use crate::remote::error::{RemoteError, RemoteResult};
use crate::remote::types::{
    now_unix, RemoteAssetRef, RemoteJobParams, RemoteJobPhase, RemoteJobRequest, RemoteJobResult,
    RemoteJobSource, RemoteJobStatus, RemoteJobSummary, REMOTE_SCHEMA_VERSION,
};
use crate::remote::upload::{
    RemoteUploadCompleteRequest, RemoteUploadInitRequest, RemoteUploadProtocol,
};

/// 同步结果：在线拉取或离线回退缓存。
#[derive(Debug, Clone)]
pub struct SyncSnapshot {
    pub online: bool,
    pub health_message: String,
    pub jobs: Vec<RemoteJobSummary>,
    pub last_sync_at: Option<u64>,
    pub from_cache: bool,
}

/// 远端转换完成结果（含本地下载路径）。
#[derive(Debug, Clone)]
pub struct RemoteConvertOutcome {
    pub status: RemoteJobStatus,
    pub result: RemoteJobResult,
    pub downloaded: Vec<PathBuf>,
}

/// 将本地转换配置映射为远端任务请求，并同步远端状态到本地缓存。
pub struct TaskSyncService {
    config: RemoteConfig,
    client: Arc<dyn RemoteClient>,
    cache: RemoteCacheStore,
}

impl TaskSyncService {
    pub fn new(config: RemoteConfig, client: Arc<dyn RemoteClient>) -> Self {
        let cache = RemoteCacheStore::new(config.resolved_cache_path());
        Self {
            config,
            client,
            cache,
        }
    }

    pub fn config(&self) -> &RemoteConfig {
        &self.config
    }

    pub fn cache_store(&self) -> &RemoteCacheStore {
        &self.cache
    }

    pub fn client(&self) -> &Arc<dyn RemoteClient> {
        &self.client
    }

    /// 仅构建请求（不上传）；保留给测试/诊断。
    pub fn build_convert_request_placeholders(&self, app: &AppConfig) -> RemoteJobRequest {
        let inputs: Vec<RemoteAssetRef> = if app.explicit_inputs.is_empty() {
            vec![RemoteAssetRef {
                id: format!("local-dir:{}", app.input_dir.display()),
                name: app
                    .input_dir
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| app.input_dir.display().to_string()),
                mime: None,
                size: None,
                checksum: None,
                download_url: None,
            }]
        } else {
            app.explicit_inputs
                .iter()
                .map(|p| RemoteAssetRef {
                    id: format!("local-file:{}", p.display()),
                    name: p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string()),
                    mime: None,
                    size: None,
                    checksum: None,
                    download_url: None,
                })
                .collect()
        };

        RemoteJobRequest {
            schema_version: REMOTE_SCHEMA_VERSION,
            source: RemoteJobSource::Convert,
            workspace_id: self.config.workspace_id.clone(),
            inputs,
            params: params_from_app(app),
            client_request_id: Some(format!("local-{}", now_unix())),
        }
    }

    /// 扫描本地输入、上传到服务器、提交真实 `RemoteAssetRef` 任务。
    pub fn submit_convert(&self, app: &AppConfig) -> RemoteResult<RemoteJobStatus> {
        self.submit_convert_with_uploads(app, None)
    }

    pub fn submit_convert_with_uploads(
        &self,
        app: &AppConfig,
        cancelled: Option<&AtomicBool>,
    ) -> RemoteResult<RemoteJobStatus> {
        if !self.config.enabled {
            return Err(RemoteError::Disabled);
        }
        if !self.config.is_configured() {
            return Err(RemoteError::NotConfigured("缺少 remote.base_url".into()));
        }

        let files = collect_input_files(app).map_err(|e| RemoteError::Other(e))?;
        if files.is_empty() {
            return Err(RemoteError::Other("没有可上传的输入文件".into()));
        }

        let mut inputs = Vec::with_capacity(files.len());
        for (idx, (abs, rel)) in files.iter().enumerate() {
            if cancelled
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(false)
            {
                return Err(RemoteError::Other("cancelled during upload".into()));
            }
            let asset = self.upload_file(abs, rel, app)?;
            tracing::info!(
                index = idx + 1,
                total = files.len(),
                asset_id = %asset.id,
                path = %abs.display(),
                "uploaded input"
            );
            inputs.push(asset);
        }

        let request = RemoteJobRequest {
            schema_version: REMOTE_SCHEMA_VERSION,
            source: RemoteJobSource::Convert,
            workspace_id: self.config.workspace_id.clone(),
            inputs,
            params: params_from_app(app),
            client_request_id: Some(format!("local-{}", now_unix())),
        };
        let status = self.client.submit_job(request)?;
        self.remember_status(&status)?;
        Ok(status)
    }

    fn upload_file(&self, abs: &Path, rel: &str, app: &AppConfig) -> RemoteResult<RemoteAssetRef> {
        let bytes = std::fs::read(abs).map_err(|e| RemoteError::Other(e.to_string()))?;
        let checksum = sha256_hex(&bytes);
        let mime = guess_mime(abs);
        let session = self.client.init_upload(RemoteUploadInitRequest {
            schema_version: REMOTE_SCHEMA_VERSION,
            file_name: rel.to_string(),
            mime: mime.clone(),
            size: Some(bytes.len() as u64),
            checksum: Some(checksum.clone()),
            protocol: RemoteUploadProtocol::PresignedPut,
            workspace_id: self
                .config
                .workspace_id
                .clone()
                .or_else(|| app.remote.workspace_id.clone()),
        })?;
        let upload_url = session
            .parts
            .first()
            .map(|p| p.upload_url.clone())
            .ok_or_else(|| RemoteError::Other("upload session missing URL".into()))?;
        if let Err(e) = self.client.upload_bytes(&upload_url, &bytes) {
            let _ = self
                .client
                .abort_upload(crate::remote::upload::RemoteUploadAbortRequest {
                    schema_version: REMOTE_SCHEMA_VERSION,
                    upload_id: session.upload_id.clone(),
                });
            return Err(e);
        }
        let done = self.client.complete_upload(RemoteUploadCompleteRequest {
            schema_version: REMOTE_SCHEMA_VERSION,
            upload_id: session.upload_id,
            checksum: Some(checksum),
            part_etags: Vec::new(),
        })?;
        // 用相对路径作为 name，便于服务端保留目录结构。
        let mut asset = done.asset;
        asset.name = rel.replace('\\', "/");
        Ok(asset)
    }

    /// 轮询直到终态。
    pub fn wait_job(
        &self,
        job_id: &str,
        poll_interval: Duration,
        cancelled: Option<&AtomicBool>,
    ) -> RemoteResult<RemoteJobStatus> {
        loop {
            if cancelled
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(false)
            {
                let _ = self.client.cancel_job(job_id);
                return Err(RemoteError::Other("cancelled while waiting".into()));
            }
            let status = self.refresh_job(job_id)?;
            if status.phase.is_terminal() {
                return Ok(status);
            }
            thread::sleep(poll_interval);
        }
    }

    /// 下载任务产物到本地输出目录。
    pub fn download_result_artifacts(
        &self,
        result: &RemoteJobResult,
        output_dir: &Path,
        overwrite: bool,
    ) -> RemoteResult<Vec<PathBuf>> {
        std::fs::create_dir_all(output_dir).map_err(|e| RemoteError::Other(e.to_string()))?;
        let mut downloaded = Vec::new();
        for asset in &result.artifacts {
            let cred = self.client.artifact_download_url(&asset.id)?;
            let bytes = self.client.download_bytes(&cred.download_url)?;
            let rel = if asset.name.is_empty() {
                asset.id.clone()
            } else {
                asset.name.replace('\\', "/")
            };
            if rel.contains("..") {
                return Err(RemoteError::Other(format!(
                    "unsafe artifact path: {}",
                    asset.name
                )));
            }
            let dest = output_dir.join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| RemoteError::Other(e.to_string()))?;
            }
            if dest.exists() && !overwrite {
                tracing::warn!(path = %dest.display(), "skip existing artifact");
                continue;
            }
            let tmp = dest.with_extension("imgforge.download");
            std::fs::write(&tmp, &bytes).map_err(|e| RemoteError::Other(e.to_string()))?;
            std::fs::rename(&tmp, &dest).map_err(|e| RemoteError::Other(e.to_string()))?;
            downloaded.push(dest);
        }
        Ok(downloaded)
    }

    /// 上传 → 提交 → 等待 → 下载，一站式远端转换。
    pub fn run_convert_and_download(
        &self,
        app: &AppConfig,
        cancelled: Option<&AtomicBool>,
    ) -> RemoteResult<RemoteConvertOutcome> {
        let status = self.submit_convert_with_uploads(app, cancelled)?;
        let status = self.wait_job(&status.job_id, Duration::from_millis(500), cancelled)?;
        let result = self.client.get_result(&status.job_id)?;
        self.remember_result(&result)?;
        let downloaded = if status.phase == RemoteJobPhase::Succeeded
            || (status.phase != RemoteJobPhase::Cancelled && !result.artifacts.is_empty())
        {
            self.download_result_artifacts(&result, &app.output_dir, app.overwrite)?
        } else {
            Vec::new()
        };
        Ok(RemoteConvertOutcome {
            status,
            result,
            downloaded,
        })
    }

    pub fn refresh_job(&self, job_id: &str) -> RemoteResult<RemoteJobStatus> {
        let status = self.client.get_job(job_id)?;
        self.remember_status(&status)?;
        if status.phase.is_terminal() {
            if let Ok(result) = self.client.get_result(job_id) {
                self.remember_result(&result)?;
            }
        }
        Ok(status)
    }

    /// 拉取远端任务列表；失败且允许离线缓存时回退本地缓存。
    pub fn sync_jobs(&self, limit: usize) -> RemoteResult<SyncSnapshot> {
        let health = self.client.health()?;
        if health.ok {
            match self.client.list_jobs(limit) {
                Ok(jobs) => {
                    let mut cache = self.load_cache();
                    cache.upsert_jobs(jobs.clone());
                    cache.record_health(true, health.message.clone());
                    if self.config.offline_cache {
                        let _ = self.cache.save(&cache);
                    }
                    return Ok(SyncSnapshot {
                        online: true,
                        health_message: health.message,
                        jobs,
                        last_sync_at: cache.last_sync_at,
                        from_cache: false,
                    });
                }
                Err(e) if self.config.offline_cache => {
                    let cache = self.load_cache();
                    return Ok(SyncSnapshot {
                        online: false,
                        health_message: format!("在线同步失败，已回退缓存：{e}"),
                        jobs: cache.jobs,
                        last_sync_at: cache.last_sync_at,
                        from_cache: true,
                    });
                }
                Err(e) => return Err(e),
            }
        }

        if self.config.offline_cache {
            let mut cache = self.load_cache();
            cache.record_health(false, health.message.clone());
            let _ = self.cache.save(&cache);
            return Ok(SyncSnapshot {
                online: false,
                health_message: health.message,
                jobs: cache.jobs,
                last_sync_at: cache.last_sync_at,
                from_cache: true,
            });
        }

        Err(RemoteError::Request(health.message))
    }

    pub fn load_cached_snapshot(&self) -> SyncSnapshot {
        let cache = self.load_cache();
        SyncSnapshot {
            online: false,
            health_message: cache
                .last_health_message
                .clone()
                .unwrap_or_else(|| "尚未同步".into()),
            jobs: cache.jobs.clone(),
            last_sync_at: cache.last_sync_at,
            from_cache: true,
        }
    }

    fn load_cache(&self) -> RemoteJobCache {
        self.cache.load().unwrap_or_default()
    }

    fn remember_status(&self, status: &RemoteJobStatus) -> RemoteResult<()> {
        if !self.config.offline_cache {
            return Ok(());
        }
        let mut cache = self.load_cache();
        let summary = RemoteJobSummary::from(status);
        if let Some(existing) = cache.jobs.iter_mut().find(|j| j.job_id == summary.job_id) {
            *existing = summary;
        } else {
            cache.jobs.insert(0, summary);
        }
        cache.last_sync_at = Some(now_unix());
        self.cache.save(&cache)
    }

    fn remember_result(&self, result: &RemoteJobResult) -> RemoteResult<()> {
        if !self.config.offline_cache {
            return Ok(());
        }
        let mut cache = self.load_cache();
        cache.replace_artifacts_for_job(&result.job_id, result.artifacts.clone());
        if let Some(job) = cache.jobs.iter_mut().find(|j| j.job_id == result.job_id) {
            job.phase = result.phase;
            job.updated_at = result.updated_at;
            job.error_summary = result.error_summary.clone();
        }
        self.cache.save(&cache)
    }
}

fn params_from_app(app: &AppConfig) -> RemoteJobParams {
    RemoteJobParams {
        target_format: Some(app.target_format.extension().to_string()),
        quality: Some(app.quality.value()),
        recursive: Some(app.recursive),
        preserve_structure: Some(app.preserve_structure),
        overwrite: Some(app.overwrite),
        rename_template: app.rename_template.clone(),
        bayer_only: Some(app.bayer_only),
        target_max_bytes: app.target_max_bytes,
        extras: Vec::new(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn guess_mime(path: &Path) -> Option<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    Some(
        match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tif" | "tiff" => "image/tiff",
            "jxl" => "image/jxl",
            "avif" => "image/avif",
            _ => "application/octet-stream",
        }
        .to_string(),
    )
}

/// 收集待上传文件：(绝对路径, 相对路径)。
fn collect_input_files(app: &AppConfig) -> Result<Vec<(PathBuf, String)>, String> {
    if !app.explicit_inputs.is_empty() {
        let mut out = Vec::new();
        for p in &app.explicit_inputs {
            if !p.is_file() {
                continue;
            }
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into());
            out.push((p.clone(), name));
        }
        return Ok(out);
    }

    let options = ScanOptions {
        input_dir: app.input_dir.clone(),
        output_dir: app.output_dir.clone(),
        target_format: app.target_format,
        recursive: app.recursive,
        preserve_structure: app.preserve_structure,
        overwrite: true, // 上传阶段不因本地输出存在而跳过
        filter: ScanFilter {
            extensions: app.extensions.clone(),
            min_size: app.min_size,
            max_size: app.max_size,
            modified_after: None,
            modified_before: None,
        },
        rename_template: None,
        bayer_only: app.bayer_only,
    };
    let tasks = crate::io::scanner::scan_inputs(&options).map_err(|e| e.to_string())?;
    let input_root = crate::io::paths::canonicalize(&app.input_dir);
    let mut out = Vec::new();
    for task in tasks {
        let rel = crate::io::paths::relative_path(&input_root, &task.input_path);
        let rel_s = rel.to_string_lossy().replace('\\', "/");
        out.push((task.input_path, rel_s));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::client::MockRemoteClient;
    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    #[test]
    fn build_request_from_app_config() {
        let client = Arc::new(MockRemoteClient::new());
        let svc = TaskSyncService::new(RemoteConfig::default(), client);
        let app = AppConfig::default();
        let req = svc.build_convert_request_placeholders(&app);
        assert_eq!(req.source, RemoteJobSource::Convert);
        assert!(!req.inputs.is_empty());
    }

    #[test]
    fn sync_falls_back_to_cache_when_offline() {
        let client = Arc::new(MockRemoteClient::new());
        let dir = tempdir().unwrap();
        let mut cfg = RemoteConfig {
            enabled: true,
            base_url: Some("http://mock".into()),
            offline_cache: true,
            ..RemoteConfig::default()
        };
        cfg.cache_path = Some(dir.path().join("cache.toml"));
        let svc = TaskSyncService::new(cfg, client.clone());
        let status = client.submit_job(RemoteJobRequest::default()).unwrap();
        svc.remember_status(&status).unwrap();
        client.set_offline(true);
        let snap = svc.sync_jobs(10).unwrap();
        assert!(snap.from_cache || !snap.online);
    }

    #[test]
    fn upload_submit_roundtrip_with_mock() {
        let client = Arc::new(MockRemoteClient::new());
        let dir = tempdir().unwrap();
        let input = dir.path().join("in");
        let output = dir.path().join("out");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let img = RgbImage::from_pixel(4, 4, Rgb([1, 2, 3]));
        let png = input.join("a.png");
        image::DynamicImage::ImageRgb8(img).save(&png).unwrap();

        let mut cfg = RemoteConfig {
            enabled: true,
            base_url: Some("http://mock".into()),
            offline_cache: false,
            ..RemoteConfig::default()
        };
        cfg.cache_path = Some(dir.path().join("c.toml"));
        let svc = TaskSyncService::new(cfg, client.clone());
        let mut app = AppConfig::default();
        app.input_dir = input;
        app.output_dir = output;
        app.explicit_inputs = vec![png];
        app.target_format = crate::core::types::ImageFormat::WebP;
        let status = svc.submit_convert(&app).unwrap();
        assert_eq!(status.phase, RemoteJobPhase::Queued);
        assert_eq!(status.total, 1);
    }
}
