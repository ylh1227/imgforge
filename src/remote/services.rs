//! 统一客户端远程 SDK：资产、任务、目录服务。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::remote::catalog::{
    RemoteAssetListItem, RemoteExtractResultSummary, RemotePage, RemotePageQuery,
    RemoteReviewBatchSummary,
};
use crate::remote::client::RemoteClient;
use crate::remote::config::RemoteConfig;
use crate::remote::error::{RemoteError, RemoteResult};
use crate::remote::models::{
    CreateRemoteBatchRequest, RemoteAnnotation, RemoteBatch, RemoteReviewItem,
    UpdateRemoteReviewItemRequest,
};
use crate::remote::types::{
    now_unix, RemoteAssetRef, RemoteJobRequest, RemoteJobResult, RemoteJobSource, RemoteJobStatus,
    RemoteJobSummary, REMOTE_SCHEMA_VERSION,
};
use crate::remote::upload::{
    RemoteUploadCompleteRequest, RemoteUploadInitRequest, RemoteUploadProtocol,
};

/// 远程资产上传 / 下载。
pub struct RemoteAssetService {
    client: Arc<dyn RemoteClient>,
    #[allow(dead_code)]
    config: RemoteConfig,
}

impl RemoteAssetService {
    pub fn new(config: RemoteConfig, client: Arc<dyn RemoteClient>) -> Self {
        Self { client, config }
    }

    pub fn upload_file(
        &self,
        path: &Path,
        workspace_id: Option<&str>,
    ) -> RemoteResult<RemoteAssetRef> {
        let bytes = std::fs::read(path).map_err(|e| RemoteError::Other(e.to_string()))?;
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "upload.bin".into());
        self.upload_bytes(&name, &bytes, workspace_id)
    }

    pub fn upload_bytes(
        &self,
        filename: &str,
        bytes: &[u8],
        workspace_id: Option<&str>,
    ) -> RemoteResult<RemoteAssetRef> {
        let checksum = format!("sha256:{:x}", Sha256::digest(bytes));
        let session = self.client.init_upload(RemoteUploadInitRequest {
            schema_version: REMOTE_SCHEMA_VERSION,
            file_name: filename.to_string(),
            size: Some(bytes.len() as u64),
            checksum: Some(checksum.clone()),
            mime: guess_mime(filename),
            workspace_id: workspace_id.map(|s| s.to_string()),
            protocol: RemoteUploadProtocol::PresignedPut,
        })?;
        let upload_url = session
            .parts
            .first()
            .map(|p| p.upload_url.clone())
            .ok_or_else(|| RemoteError::Other("upload session missing url".into()))?;
        self.client.upload_bytes(&upload_url, bytes)?;
        let complete = self.client.complete_upload(RemoteUploadCompleteRequest {
            schema_version: REMOTE_SCHEMA_VERSION,
            upload_id: session.upload_id,
            part_etags: Vec::new(),
            checksum: Some(checksum),
        })?;
        Ok(complete.asset)
    }

    pub fn download_to(&self, asset_id: &str, dest: &Path) -> RemoteResult<PathBuf> {
        let cred = self.client.artifact_download_url(asset_id)?;
        let bytes = self.client.download_bytes(&cred.download_url)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RemoteError::Other(e.to_string()))?;
        }
        std::fs::write(dest, bytes).map_err(|e| RemoteError::Other(e.to_string()))?;
        Ok(dest.to_path_buf())
    }
}

/// 远程任务提交 / 等待 / 取消。
pub struct RemoteJobService {
    client: Arc<dyn RemoteClient>,
}

impl RemoteJobService {
    pub fn new(client: Arc<dyn RemoteClient>) -> Self {
        Self { client }
    }

    pub fn submit(&self, request: RemoteJobRequest) -> RemoteResult<RemoteJobStatus> {
        self.client.submit_job(request)
    }

    pub fn get(&self, job_id: &str) -> RemoteResult<RemoteJobStatus> {
        self.client.get_job(job_id)
    }

    pub fn list(&self, limit: usize) -> RemoteResult<Vec<RemoteJobSummary>> {
        self.client.list_jobs(limit)
    }

    pub fn result(&self, job_id: &str) -> RemoteResult<RemoteJobResult> {
        self.client.get_result(job_id)
    }

    pub fn cancel(&self, job_id: &str) -> RemoteResult<RemoteJobStatus> {
        self.client.cancel_job(job_id)
    }

    pub fn wait(
        &self,
        job_id: &str,
        poll_interval: Duration,
        cancel: Option<&AtomicBool>,
    ) -> RemoteResult<RemoteJobStatus> {
        loop {
            if cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                let _ = self.cancel(job_id);
                return Err(RemoteError::Other("cancelled".into()));
            }
            let status = self.get(job_id)?;
            if status.phase.is_terminal() {
                return Ok(status);
            }
            thread::sleep(poll_interval);
        }
    }

    pub fn submit_and_wait(
        &self,
        request: RemoteJobRequest,
        poll_interval: Duration,
        cancel: Option<&AtomicBool>,
    ) -> RemoteResult<(RemoteJobStatus, RemoteJobResult)> {
        let submitted = self.submit(request)?;
        let status = self.wait(&submitted.job_id, poll_interval, cancel)?;
        let result = self.result(&submitted.job_id)?;
        Ok((status, result))
    }
}

/// 远程目录：批次、条目、标注、提取结果。
pub struct RemoteCatalogService {
    client: Arc<dyn RemoteClient>,
}

impl RemoteCatalogService {
    pub fn new(client: Arc<dyn RemoteClient>) -> Self {
        Self { client }
    }

    pub fn list_assets(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteAssetListItem>> {
        self.client.list_assets(query)
    }

    pub fn list_review_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        self.client.list_review_batches(query)
    }

    pub fn list_video_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        self.client.list_video_batches(query)
    }

    pub fn list_extract_results(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteExtractResultSummary>> {
        self.client.list_extract_results(query)
    }

    pub fn create_batch(&self, request: CreateRemoteBatchRequest) -> RemoteResult<RemoteBatch> {
        self.client.create_batch(request)
    }

    pub fn get_batch(&self, batch_id: &str) -> RemoteResult<RemoteBatch> {
        self.client.get_batch(batch_id)
    }

    pub fn list_review_items(&self, batch_id: &str) -> RemoteResult<Vec<RemoteReviewItem>> {
        self.client.list_review_items(batch_id)
    }

    pub fn update_review_item(
        &self,
        item_id: &str,
        request: UpdateRemoteReviewItemRequest,
    ) -> RemoteResult<RemoteReviewItem> {
        self.client.update_review_item(item_id, request)
    }

    pub fn upsert_annotation(
        &self,
        annotation: RemoteAnnotation,
    ) -> RemoteResult<RemoteAnnotation> {
        self.client.upsert_annotation(annotation)
    }

    pub fn list_annotations(&self, item_id: &str) -> RemoteResult<Vec<RemoteAnnotation>> {
        self.client.list_annotations(item_id)
    }
}

fn guess_mime(name: &str) -> Option<String> {
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "png" => Some("image/png".into()),
        "webp" => Some("image/webp".into()),
        "gif" => Some("image/gif".into()),
        "mp4" => Some("video/mp4".into()),
        "mov" => Some("video/quicktime".into()),
        "csv" => Some("text/csv".into()),
        "json" => Some("application/json".into()),
        "pdf" => Some("application/pdf".into()),
        _ => None,
    }
}

/// 便捷：按 source 构造最小 job 请求。
pub fn job_request(
    source: RemoteJobSource,
    inputs: Vec<RemoteAssetRef>,
    workspace_id: Option<String>,
) -> RemoteJobRequest {
    RemoteJobRequest {
        schema_version: REMOTE_SCHEMA_VERSION,
        source,
        workspace_id,
        inputs,
        params: Default::default(),
        client_request_id: Some(format!("req-{}", now_unix())),
    }
}
