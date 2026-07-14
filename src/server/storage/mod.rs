//! 任务元数据存储抽象（内存 + SQLite + Postgres）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::remote::catalog::{
    RemoteAssetListItem, RemoteExtractResultSummary, RemotePage, RemotePageQuery,
    RemoteReviewBatchSummary,
};
use crate::remote::contract::{RemoteApiErrorBody, RemoteApiErrorCode};
use crate::remote::events::RemoteJobEvent;
use crate::remote::models::{
    CreateRemoteBatchRequest, RemoteAnnotation, RemoteBatch, RemoteBatchKind, RemoteReviewItem,
    RemoteReviewItemStatus, UpdateRemoteReviewItemRequest,
};
use crate::remote::types::{
    now_unix, RemoteAssetRef, RemoteJobPhase, RemoteJobRequest, RemoteJobResult, RemoteJobSource,
    RemoteJobStatus, RemoteJobSummary, REMOTE_SCHEMA_VERSION,
};
use crate::remote::upload::{
    RemoteDownloadCredential, RemoteUploadCompleteRequest, RemoteUploadCompleteResponse,
    RemoteUploadInitRequest, RemoteUploadSession,
};
use crate::remote::worker_policy::{DeadLetterRecord, IdempotencyKey};

#[cfg(feature = "server")]
mod postgres;
#[cfg(feature = "server")]
pub use postgres::PostgresJobStore;

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone)]
pub enum StoreError {
    NotFound(String),
    Conflict(String),
    Validation(String),
    Internal(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(m) => write!(f, "not found: {m}"),
            Self::Conflict(m) => write!(f, "conflict: {m}"),
            Self::Validation(m) => write!(f, "validation: {m}"),
            Self::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl StoreError {
    pub fn into_api(self) -> RemoteApiErrorBody {
        match self {
            Self::NotFound(m) => RemoteApiErrorBody::new(RemoteApiErrorCode::NotFound, m),
            Self::Conflict(m) => RemoteApiErrorBody::new(RemoteApiErrorCode::Conflict, m),
            Self::Validation(m) => RemoteApiErrorBody::new(RemoteApiErrorCode::Validation, m),
            Self::Internal(m) => {
                RemoteApiErrorBody::new(RemoteApiErrorCode::Internal, m).retryable(true)
            }
        }
    }
}

/// 完整任务记录（含原始请求，供 Worker 使用）。
#[derive(Debug, Clone)]
pub struct RemoteJobRecord {
    pub status: RemoteJobStatus,
    pub request: RemoteJobRequest,
    pub attempt: u32,
}

/// 上传会话内部状态。
#[derive(Debug, Clone)]
pub struct UploadRecord {
    pub init: RemoteUploadInitRequest,
    pub object_key: String,
    pub bytes_received: Option<u64>,
    pub received_checksum: Option<String>,
    pub completed: bool,
}

pub trait JobStore: Send + Sync {
    fn create_job(&self, request: RemoteJobRequest) -> StoreResult<RemoteJobStatus>;
    fn get_job(&self, job_id: &str) -> StoreResult<RemoteJobStatus>;
    fn get_job_record(&self, job_id: &str) -> StoreResult<RemoteJobRecord>;
    fn list_jobs(&self, limit: usize) -> StoreResult<Vec<RemoteJobSummary>>;
    fn update_job(&self, status: RemoteJobStatus) -> StoreResult<()>;
    fn set_result(&self, result: RemoteJobResult) -> StoreResult<()>;
    fn get_result(&self, job_id: &str) -> StoreResult<RemoteJobResult>;
    fn append_event(&self, event: RemoteJobEvent) -> StoreResult<()>;
    fn list_events(&self, job_id: &str, after_ts: Option<u64>) -> StoreResult<Vec<RemoteJobEvent>>;
    fn init_upload(
        &self,
        request: RemoteUploadInitRequest,
        public_base: &str,
    ) -> StoreResult<RemoteUploadSession>;
    fn get_upload(&self, upload_id: &str) -> StoreResult<UploadRecord>;
    fn mark_upload_bytes(
        &self,
        upload_id: &str,
        size: u64,
        checksum: Option<String>,
    ) -> StoreResult<()>;
    fn complete_upload(
        &self,
        request: RemoteUploadCompleteRequest,
        public_base: &str,
    ) -> StoreResult<RemoteUploadCompleteResponse>;
    fn abort_upload(&self, upload_id: &str) -> StoreResult<()>;
    fn register_asset(&self, asset: RemoteAssetRef) -> StoreResult<()>;
    fn get_asset(&self, asset_id: &str) -> StoreResult<RemoteAssetRef>;
    fn download_credential(
        &self,
        asset_id: &str,
        public_base: &str,
    ) -> StoreResult<RemoteDownloadCredential>;
    fn list_assets(&self, query: RemotePageQuery) -> StoreResult<RemotePage<RemoteAssetListItem>>;
    fn list_review_batches(
        &self,
        query: RemotePageQuery,
    ) -> StoreResult<RemotePage<RemoteReviewBatchSummary>>;
    fn list_extract_results(
        &self,
        query: RemotePageQuery,
    ) -> StoreResult<RemotePage<RemoteExtractResultSummary>>;
    fn put_dead_letter(&self, record: DeadLetterRecord) -> StoreResult<()>;
    fn list_dead_letters(&self) -> StoreResult<Vec<DeadLetterRecord>>;
    /// 列出仍处于 Queued 的 job_id（启动恢复用）。
    fn list_queued_job_ids(&self) -> StoreResult<Vec<String>> {
        Ok(self
            .list_jobs(10_000)?
            .into_iter()
            .filter(|j| j.phase == RemoteJobPhase::Queued)
            .map(|j| j.job_id)
            .collect())
    }

    fn create_batch(&self, request: CreateRemoteBatchRequest) -> StoreResult<RemoteBatch> {
        let _ = request;
        Err(StoreError::Internal(
            "create_batch not implemented for this store".into(),
        ))
    }
    fn get_batch(&self, batch_id: &str) -> StoreResult<RemoteBatch> {
        let _ = batch_id;
        Err(StoreError::NotFound("batch".into()))
    }
    fn list_review_items(&self, batch_id: &str) -> StoreResult<Vec<RemoteReviewItem>> {
        let _ = batch_id;
        Ok(Vec::new())
    }
    fn get_review_item(&self, item_id: &str) -> StoreResult<RemoteReviewItem> {
        let _ = item_id;
        Err(StoreError::NotFound("item".into()))
    }
    fn update_review_item(
        &self,
        item_id: &str,
        request: UpdateRemoteReviewItemRequest,
    ) -> StoreResult<RemoteReviewItem> {
        let _ = (item_id, request);
        Err(StoreError::NotFound("item".into()))
    }
    fn update_review_item_assets(
        &self,
        item_id: &str,
        thumb_asset: Option<RemoteAssetRef>,
        preview_asset: Option<RemoteAssetRef>,
        duration_ms: Option<u64>,
        dimensions: Option<(u32, u32)>,
    ) -> StoreResult<RemoteReviewItem> {
        let _ = (item_id, thumb_asset, preview_asset, duration_ms, dimensions);
        Err(StoreError::NotFound("item".into()))
    }
    fn upsert_annotation(&self, annotation: RemoteAnnotation) -> StoreResult<RemoteAnnotation> {
        let _ = annotation;
        Err(StoreError::Internal(
            "upsert_annotation not implemented".into(),
        ))
    }
    fn list_annotations(&self, item_id: &str) -> StoreResult<Vec<RemoteAnnotation>> {
        let _ = item_id;
        Ok(Vec::new())
    }
    fn get_annotation(&self, annotation_id: &str) -> StoreResult<RemoteAnnotation> {
        let _ = annotation_id;
        Err(StoreError::NotFound("annotation".into()))
    }
    fn upsert_extract_result(
        &self,
        summary: RemoteExtractResultSummary,
    ) -> StoreResult<RemoteExtractResultSummary> {
        let _ = summary;
        Err(StoreError::Internal(
            "upsert_extract_result not implemented".into(),
        ))
    }
    fn append_audit(
        &self,
        workspace_id: Option<&str>,
        actor: Option<&str>,
        action: &str,
        detail: Option<&str>,
    ) -> StoreResult<()> {
        let _ = (workspace_id, actor, action, detail);
        Ok(())
    }
}

#[derive(Default)]
struct MemoryState {
    jobs: HashMap<String, RemoteJobStatus>,
    requests: HashMap<String, RemoteJobRequest>,
    attempts: HashMap<String, u32>,
    results: HashMap<String, RemoteJobResult>,
    events: HashMap<String, Vec<RemoteJobEvent>>,
    uploads: HashMap<String, UploadRecord>,
    assets: HashMap<String, RemoteAssetRef>,
    idempotency: HashMap<IdempotencyKey, String>,
    dead_letters: Vec<DeadLetterRecord>,
    review_batches: Vec<RemoteReviewBatchSummary>,
    batches: HashMap<String, RemoteBatch>,
    review_items: HashMap<String, RemoteReviewItem>,
    annotations: HashMap<String, RemoteAnnotation>,
    extract_results: Vec<RemoteExtractResultSummary>,
    next_id: u64,
}

pub struct InMemoryJobStore {
    inner: Arc<Mutex<MemoryState>>,
}

impl InMemoryJobStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemoryState::default())),
        }
    }

    fn lock(&self) -> StoreResult<std::sync::MutexGuard<'_, MemoryState>> {
        self.inner
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))
    }
}

impl Default for InMemoryJobStore {
    fn default() -> Self {
        Self::new()
    }
}

fn create_job_inner(
    g: &mut MemoryState,
    request: RemoteJobRequest,
) -> StoreResult<RemoteJobStatus> {
    if let Some(client_request_id) = request.client_request_id.as_ref() {
        let key = IdempotencyKey::new(
            request
                .workspace_id
                .clone()
                .unwrap_or_else(|| "default".into()),
            client_request_id.clone(),
        );
        if let Some(existing) = g.idempotency.get(&key) {
            return g
                .jobs
                .get(existing)
                .cloned()
                .ok_or_else(|| StoreError::Internal("idempotent job missing".into()));
        }
        g.next_id += 1;
        let job_id = format!("job-{}", g.next_id);
        let mut status =
            RemoteJobStatus::new(job_id.clone(), request.source, RemoteJobPhase::Queued);
        status.total = request.inputs.len().max(1);
        let event = RemoteJobEvent::phase_change(
            &job_id,
            request.source,
            RemoteJobPhase::Queued,
            now_unix(),
        );
        g.events.entry(job_id.clone()).or_default().push(event);
        g.idempotency.insert(key, job_id.clone());
        g.requests.insert(job_id.clone(), request);
        g.attempts.insert(job_id.clone(), 0);
        g.jobs.insert(job_id, status.clone());
        return Ok(status);
    }

    g.next_id += 1;
    let job_id = format!("job-{}", g.next_id);
    let mut status = RemoteJobStatus::new(job_id.clone(), request.source, RemoteJobPhase::Queued);
    status.total = request.inputs.len().max(1);
    let event =
        RemoteJobEvent::phase_change(&job_id, request.source, RemoteJobPhase::Queued, now_unix());
    g.events.entry(job_id.clone()).or_default().push(event);
    g.requests.insert(job_id.clone(), request);
    g.attempts.insert(job_id.clone(), 0);
    g.jobs.insert(job_id, status.clone());
    Ok(status)
}

impl JobStore for InMemoryJobStore {
    fn create_job(&self, request: RemoteJobRequest) -> StoreResult<RemoteJobStatus> {
        let mut g = self.lock()?;
        create_job_inner(&mut g, request)
    }

    fn get_job(&self, job_id: &str) -> StoreResult<RemoteJobStatus> {
        let g = self.lock()?;
        g.jobs
            .get(job_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(job_id.into()))
    }

    fn get_job_record(&self, job_id: &str) -> StoreResult<RemoteJobRecord> {
        let g = self.lock()?;
        let status = g
            .jobs
            .get(job_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(job_id.into()))?;
        let request = g
            .requests
            .get(job_id)
            .cloned()
            .ok_or_else(|| StoreError::Internal(format!("missing request for {job_id}")))?;
        let attempt = g.attempts.get(job_id).copied().unwrap_or(0);
        Ok(RemoteJobRecord {
            status,
            request,
            attempt,
        })
    }

    fn list_jobs(&self, limit: usize) -> StoreResult<Vec<RemoteJobSummary>> {
        let g = self.lock()?;
        let mut jobs: Vec<_> = g.jobs.values().map(RemoteJobSummary::from).collect();
        jobs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        jobs.truncate(limit.max(1));
        Ok(jobs)
    }

    fn update_job(&self, status: RemoteJobStatus) -> StoreResult<()> {
        let mut g = self.lock()?;
        if !g.jobs.contains_key(&status.job_id) {
            return Err(StoreError::NotFound(status.job_id));
        }
        let event = RemoteJobEvent::phase_change(
            &status.job_id,
            status.source,
            status.phase,
            status.updated_at,
        );
        g.events
            .entry(status.job_id.clone())
            .or_default()
            .push(event);
        g.jobs.insert(status.job_id.clone(), status);
        Ok(())
    }

    fn set_result(&self, result: RemoteJobResult) -> StoreResult<()> {
        let mut g = self.lock()?;
        g.results.insert(result.job_id.clone(), result);
        Ok(())
    }

    fn get_result(&self, job_id: &str) -> StoreResult<RemoteJobResult> {
        let g = self.lock()?;
        g.results
            .get(job_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(job_id.into()))
    }

    fn append_event(&self, event: RemoteJobEvent) -> StoreResult<()> {
        let mut g = self.lock()?;
        g.events
            .entry(event.job_id.clone())
            .or_default()
            .push(event);
        Ok(())
    }

    fn list_events(&self, job_id: &str, after_ts: Option<u64>) -> StoreResult<Vec<RemoteJobEvent>> {
        let g = self.lock()?;
        let events = g.events.get(job_id).cloned().unwrap_or_default();
        Ok(events
            .into_iter()
            .filter(|e| after_ts.map(|ts| e.ts > ts).unwrap_or(true))
            .collect())
    }

    fn init_upload(
        &self,
        request: RemoteUploadInitRequest,
        public_base: &str,
    ) -> StoreResult<RemoteUploadSession> {
        if request.file_name.trim().is_empty() {
            return Err(StoreError::Validation("file_name required".into()));
        }
        let mut g = self.lock()?;
        g.next_id += 1;
        let upload_id = format!("upl-{}", g.next_id);
        let part_size = request.size.unwrap_or(8 * 1024 * 1024).max(1);
        let object_key = format!("uploads/{upload_id}");
        let url = format!(
            "{}/v1/uploads/{}/bytes",
            public_base.trim_end_matches('/'),
            upload_id
        );
        g.uploads.insert(
            upload_id.clone(),
            UploadRecord {
                init: request,
                object_key,
                bytes_received: None,
                received_checksum: None,
                completed: false,
            },
        );
        Ok(RemoteUploadSession::single_put(
            upload_id, url, part_size, 3600,
        ))
    }

    fn get_upload(&self, upload_id: &str) -> StoreResult<UploadRecord> {
        let g = self.lock()?;
        g.uploads
            .get(upload_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(upload_id.into()))
    }

    fn mark_upload_bytes(
        &self,
        upload_id: &str,
        size: u64,
        checksum: Option<String>,
    ) -> StoreResult<()> {
        let mut g = self.lock()?;
        let rec = g
            .uploads
            .get_mut(upload_id)
            .ok_or_else(|| StoreError::NotFound(upload_id.into()))?;
        if rec.completed {
            return Err(StoreError::Conflict("upload already completed".into()));
        }
        if let Some(expected) = rec.init.size {
            if expected != size {
                return Err(StoreError::Validation(format!(
                    "size mismatch: expected {expected}, got {size}"
                )));
            }
        }
        if let (Some(expected), Some(actual)) = (rec.init.checksum.as_ref(), checksum.as_ref()) {
            if expected != actual {
                return Err(StoreError::Validation("checksum mismatch".into()));
            }
        }
        rec.bytes_received = Some(size);
        rec.received_checksum = checksum;
        Ok(())
    }

    fn complete_upload(
        &self,
        request: RemoteUploadCompleteRequest,
        public_base: &str,
    ) -> StoreResult<RemoteUploadCompleteResponse> {
        let mut g = self.lock()?;
        let rec = g
            .uploads
            .get_mut(&request.upload_id)
            .ok_or_else(|| StoreError::NotFound(request.upload_id.clone()))?;
        if rec.bytes_received.is_none() {
            return Err(StoreError::Validation(
                "upload bytes not received; PUT /v1/uploads/{id}/bytes first".into(),
            ));
        }
        let checksum = request
            .checksum
            .clone()
            .or_else(|| rec.received_checksum.clone())
            .or_else(|| rec.init.checksum.clone());
        if let (Some(expected), Some(actual)) = (rec.init.checksum.as_ref(), checksum.as_ref()) {
            if expected != actual {
                return Err(StoreError::Validation("checksum mismatch".into()));
            }
        }
        rec.completed = true;
        let asset_id = format!("asset-{}", request.upload_id);
        let asset = RemoteAssetRef {
            id: asset_id.clone(),
            name: rec.init.file_name.clone(),
            mime: rec.init.mime.clone(),
            size: rec.bytes_received.or(rec.init.size),
            checksum,
            download_url: Some(format!(
                "{}/v1/artifacts/{}/content",
                public_base.trim_end_matches('/'),
                asset_id
            )),
        };
        // 对象键：完成时从 uploads/{id} 逻辑映射到 assets/{asset_id}（由 API 层搬迁文件）。
        g.assets.insert(asset_id, asset.clone());
        Ok(RemoteUploadCompleteResponse {
            schema_version: REMOTE_SCHEMA_VERSION,
            asset,
        })
    }

    fn abort_upload(&self, upload_id: &str) -> StoreResult<()> {
        let mut g = self.lock()?;
        g.uploads.remove(upload_id);
        Ok(())
    }

    fn register_asset(&self, asset: RemoteAssetRef) -> StoreResult<()> {
        let mut g = self.lock()?;
        g.assets.insert(asset.id.clone(), asset);
        Ok(())
    }

    fn get_asset(&self, asset_id: &str) -> StoreResult<RemoteAssetRef> {
        let g = self.lock()?;
        g.assets
            .get(asset_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(asset_id.into()))
    }

    fn download_credential(
        &self,
        asset_id: &str,
        public_base: &str,
    ) -> StoreResult<RemoteDownloadCredential> {
        let g = self.lock()?;
        let asset = g
            .assets
            .get(asset_id)
            .ok_or_else(|| StoreError::NotFound(asset_id.into()))?;
        Ok(RemoteDownloadCredential {
            schema_version: REMOTE_SCHEMA_VERSION,
            asset_id: asset.id.clone(),
            download_url: asset.download_url.clone().unwrap_or_else(|| {
                format!(
                    "{}/v1/artifacts/{}/content",
                    public_base.trim_end_matches('/'),
                    asset_id
                )
            }),
            expires_at: now_unix() + 3600,
            checksum: asset.checksum.clone(),
        })
    }

    fn list_assets(&self, query: RemotePageQuery) -> StoreResult<RemotePage<RemoteAssetListItem>> {
        let g = self.lock()?;
        let mut items: Vec<_> = g
            .assets
            .values()
            .map(|a| RemoteAssetListItem {
                asset: a.clone(),
                created_at: Some(now_unix()),
                tags: None,
            })
            .collect();
        let total = items.len();
        items.sort_by(|a, b| a.asset.id.cmp(&b.asset.id));
        let page = items
            .into_iter()
            .skip(query.offset)
            .take(query.limit.max(1))
            .collect();
        Ok(RemotePage::new(page, total))
    }

    fn list_review_batches(
        &self,
        query: RemotePageQuery,
    ) -> StoreResult<RemotePage<RemoteReviewBatchSummary>> {
        let g = self.lock()?;
        let total = g.review_batches.len();
        let page = g
            .review_batches
            .iter()
            .skip(query.offset)
            .take(query.limit.max(1))
            .cloned()
            .collect();
        Ok(RemotePage::new(page, total))
    }

    fn list_extract_results(
        &self,
        query: RemotePageQuery,
    ) -> StoreResult<RemotePage<RemoteExtractResultSummary>> {
        let g = self.lock()?;
        let total = g.extract_results.len();
        let page = g
            .extract_results
            .iter()
            .skip(query.offset)
            .take(query.limit.max(1))
            .cloned()
            .collect();
        Ok(RemotePage::new(page, total))
    }

    fn put_dead_letter(&self, record: DeadLetterRecord) -> StoreResult<()> {
        let mut g = self.lock()?;
        g.dead_letters.push(record);
        Ok(())
    }

    fn list_dead_letters(&self) -> StoreResult<Vec<DeadLetterRecord>> {
        let g = self.lock()?;
        Ok(g.dead_letters.clone())
    }

    fn create_batch(&self, request: CreateRemoteBatchRequest) -> StoreResult<RemoteBatch> {
        let mut g = self.lock()?;
        g.next_id += 1;
        let batch_id = format!("batch-{}", g.next_id);
        let now = now_unix();
        let source = match request.kind {
            RemoteBatchKind::Image => RemoteJobSource::Review,
            RemoteBatchKind::Video => RemoteJobSource::VideoReview,
        };
        let mut items = Vec::new();
        let mut cover = None;
        for asset_id in &request.asset_ids {
            let asset = g
                .assets
                .get(asset_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("asset {asset_id}")))?;
            if cover.is_none() {
                cover = Some(asset.clone());
            }
            g.next_id += 1;
            let item_id = format!("item-{}", g.next_id);
            let item = RemoteReviewItem {
                schema_version: REMOTE_SCHEMA_VERSION,
                item_id: item_id.clone(),
                batch_id: batch_id.clone(),
                asset,
                status: RemoteReviewItemStatus::Pending,
                remark: String::new(),
                tags: Vec::new(),
                thumb_asset: None,
                preview_asset: None,
                duration_ms: None,
                width: None,
                height: None,
                updated_at: now,
            };
            g.review_items.insert(item_id, item.clone());
            items.push(item);
        }
        let batch = RemoteBatch {
            schema_version: REMOTE_SCHEMA_VERSION,
            batch_id: batch_id.clone(),
            name: request.name,
            kind: request.kind,
            source,
            workspace_id: request.workspace_id,
            item_count: items.len(),
            created_at: now,
            updated_at: now,
            cover_asset: cover.clone(),
            status_summary: Some("pending".into()),
        };
        g.batches.insert(batch_id.clone(), batch.clone());
        g.review_batches.push(RemoteReviewBatchSummary {
            batch_id,
            name: batch.name.clone(),
            source,
            item_count: batch.item_count,
            updated_at: now,
            cover_asset: cover,
        });
        Ok(batch)
    }

    fn get_batch(&self, batch_id: &str) -> StoreResult<RemoteBatch> {
        let g = self.lock()?;
        g.batches
            .get(batch_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(batch_id.into()))
    }

    fn list_review_items(&self, batch_id: &str) -> StoreResult<Vec<RemoteReviewItem>> {
        let g = self.lock()?;
        let mut items: Vec<_> = g
            .review_items
            .values()
            .filter(|i| i.batch_id == batch_id)
            .cloned()
            .collect();
        items.sort_by(|a, b| a.item_id.cmp(&b.item_id));
        Ok(items)
    }

    fn get_review_item(&self, item_id: &str) -> StoreResult<RemoteReviewItem> {
        let g = self.lock()?;
        g.review_items
            .get(item_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(item_id.into()))
    }

    fn update_review_item(
        &self,
        item_id: &str,
        request: UpdateRemoteReviewItemRequest,
    ) -> StoreResult<RemoteReviewItem> {
        let mut g = self.lock()?;
        let item = g
            .review_items
            .get_mut(item_id)
            .ok_or_else(|| StoreError::NotFound(item_id.into()))?;
        if let Some(status) = request.status {
            item.status = status;
        }
        if let Some(remark) = request.remark {
            item.remark = remark;
        }
        if let Some(tags) = request.tags {
            item.tags = tags;
        }
        item.updated_at = now_unix();
        let out = item.clone();
        if let Some(batch) = g.batches.get_mut(&out.batch_id) {
            batch.updated_at = out.updated_at;
        }
        if let Some(summary) = g
            .review_batches
            .iter_mut()
            .find(|b| b.batch_id == out.batch_id)
        {
            summary.updated_at = out.updated_at;
        }
        Ok(out)
    }

    fn update_review_item_assets(
        &self,
        item_id: &str,
        thumb_asset: Option<RemoteAssetRef>,
        preview_asset: Option<RemoteAssetRef>,
        duration_ms: Option<u64>,
        dimensions: Option<(u32, u32)>,
    ) -> StoreResult<RemoteReviewItem> {
        let mut g = self.lock()?;
        let item = g
            .review_items
            .get_mut(item_id)
            .ok_or_else(|| StoreError::NotFound(item_id.into()))?;
        if let Some(asset) = thumb_asset {
            item.thumb_asset = Some(asset);
        }
        if let Some(asset) = preview_asset {
            item.preview_asset = Some(asset);
        }
        if let Some(duration_ms) = duration_ms {
            item.duration_ms = Some(duration_ms);
        }
        if let Some((width, height)) = dimensions {
            item.width = Some(width);
            item.height = Some(height);
        }
        item.updated_at = now_unix();
        let out = item.clone();
        if let Some(batch) = g.batches.get_mut(&out.batch_id) {
            batch.updated_at = out.updated_at;
            if batch.cover_asset.is_none() {
                batch.cover_asset = out
                    .thumb_asset
                    .clone()
                    .or_else(|| out.preview_asset.clone())
                    .or_else(|| Some(out.asset.clone()));
            }
        }
        if let Some(summary) = g
            .review_batches
            .iter_mut()
            .find(|b| b.batch_id == out.batch_id)
        {
            summary.updated_at = out.updated_at;
            if summary.cover_asset.is_none() {
                summary.cover_asset = out
                    .thumb_asset
                    .clone()
                    .or_else(|| out.preview_asset.clone())
                    .or_else(|| Some(out.asset.clone()));
            }
        }
        Ok(out)
    }

    fn upsert_annotation(&self, annotation: RemoteAnnotation) -> StoreResult<RemoteAnnotation> {
        let mut g = self.lock()?;
        let mut ann = annotation;
        if ann.annotation_id.is_empty() {
            g.next_id += 1;
            ann.annotation_id = format!("ann-{}", g.next_id);
        }
        if ann.schema_version == 0 {
            ann.schema_version = REMOTE_SCHEMA_VERSION;
        }
        if ann.created_at == 0 {
            ann.created_at = now_unix();
        }
        g.annotations.insert(ann.annotation_id.clone(), ann.clone());
        Ok(ann)
    }

    fn list_annotations(&self, item_id: &str) -> StoreResult<Vec<RemoteAnnotation>> {
        let g = self.lock()?;
        let mut items: Vec<_> = g
            .annotations
            .values()
            .filter(|a| a.item_id == item_id)
            .cloned()
            .collect();
        items.sort_by(|a, b| a.annotation_id.cmp(&b.annotation_id));
        Ok(items)
    }

    fn get_annotation(&self, annotation_id: &str) -> StoreResult<RemoteAnnotation> {
        let g = self.lock()?;
        g.annotations
            .get(annotation_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(annotation_id.into()))
    }

    fn upsert_extract_result(
        &self,
        summary: RemoteExtractResultSummary,
    ) -> StoreResult<RemoteExtractResultSummary> {
        let mut g = self.lock()?;
        if let Some(existing) = g
            .extract_results
            .iter_mut()
            .find(|r| r.result_id == summary.result_id)
        {
            *existing = summary.clone();
        } else {
            g.extract_results.push(summary.clone());
        }
        Ok(summary)
    }
}

impl InMemoryJobStore {
    pub(crate) fn import_job(
        &self,
        status: RemoteJobStatus,
        request: RemoteJobRequest,
        attempt: u32,
    ) -> StoreResult<()> {
        let mut g = self.lock()?;
        let job_id = status.job_id.clone();
        g.jobs.insert(job_id.clone(), status);
        g.requests.insert(job_id.clone(), request);
        g.attempts.insert(job_id, attempt);
        Ok(())
    }

    pub(crate) fn import_batch(&self, batch: RemoteBatch) -> StoreResult<()> {
        let mut g = self.lock()?;
        let summary = RemoteReviewBatchSummary {
            batch_id: batch.batch_id.clone(),
            name: batch.name.clone(),
            source: batch.source,
            item_count: batch.item_count,
            updated_at: batch.updated_at,
            cover_asset: batch.cover_asset.clone(),
        };
        if let Some(existing) = g
            .review_batches
            .iter_mut()
            .find(|b| b.batch_id == batch.batch_id)
        {
            *existing = summary;
        } else {
            g.review_batches.push(summary);
        }
        g.batches.insert(batch.batch_id.clone(), batch);
        Ok(())
    }

    pub(crate) fn import_review_item(&self, item: RemoteReviewItem) -> StoreResult<()> {
        let mut g = self.lock()?;
        g.review_items.insert(item.item_id.clone(), item);
        Ok(())
    }

    pub(crate) fn import_annotation(&self, annotation: RemoteAnnotation) -> StoreResult<()> {
        let mut g = self.lock()?;
        g.annotations
            .insert(annotation.annotation_id.clone(), annotation);
        Ok(())
    }

    pub(crate) fn import_extract_result(
        &self,
        summary: RemoteExtractResultSummary,
    ) -> StoreResult<()> {
        self.upsert_extract_result(summary)?;
        Ok(())
    }
}

/// SQLite 持久化 JobStore：元数据落盘，对象字节仍由 ObjectStore 管理。
pub struct SqliteJobStore {
    conn: Mutex<rusqlite::Connection>,
    /// 与内存实现共享的热路径缓存（简化：直接用内存镜像 + 落盘）。
    memory: InMemoryJobStore,
    path: PathBuf,
}

impl SqliteJobStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        let conn =
            rusqlite::Connection::open(&path).map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS jobs (
              job_id TEXT PRIMARY KEY,
              status_json TEXT NOT NULL,
              request_json TEXT NOT NULL,
              attempt INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS results (
              job_id TEXT PRIMARY KEY,
              result_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              job_id TEXT NOT NULL,
              ts INTEGER NOT NULL,
              event_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS uploads (
              upload_id TEXT PRIMARY KEY,
              record_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS assets (
              asset_id TEXT PRIMARY KEY,
              asset_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS idempotency (
              workspace_id TEXT NOT NULL,
              client_request_id TEXT NOT NULL,
              job_id TEXT NOT NULL,
              PRIMARY KEY (workspace_id, client_request_id)
            );
            CREATE TABLE IF NOT EXISTS dead_letters (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              record_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS review_batches (
              batch_id TEXT PRIMARY KEY,
              batch_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS review_items (
              item_id TEXT PRIMARY KEY,
              batch_id TEXT NOT NULL,
              item_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS annotations (
              annotation_id TEXT PRIMARY KEY,
              item_id TEXT NOT NULL,
              annotation_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS extract_results (
              result_id TEXT PRIMARY KEY,
              result_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_log (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              ts INTEGER NOT NULL,
              workspace_id TEXT,
              actor TEXT,
              action TEXT NOT NULL,
              detail TEXT
            );
            "#,
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;

        let store = Self {
            conn: Mutex::new(conn),
            memory: InMemoryJobStore::new(),
            path,
        };
        store.load_into_memory()?;
        Ok(store)
    }

    fn load_into_memory(&self) -> StoreResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        let mut g = self.memory.lock()?;

        {
            let mut stmt = conn
                .prepare("SELECT job_id, status_json, request_json, attempt FROM jobs")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u32>(3)?,
                    ))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (job_id, status_json, request_json, attempt) =
                    row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let status: RemoteJobStatus = serde_json::from_str(&status_json)
                    .map_err(|e| StoreError::Internal(e.to_string()))?;
                let request: RemoteJobRequest = serde_json::from_str(&request_json)
                    .map_err(|e| StoreError::Internal(e.to_string()))?;
                g.jobs.insert(job_id.clone(), status);
                g.requests.insert(job_id.clone(), request);
                g.attempts.insert(job_id, attempt);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT job_id, result_json FROM results")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (job_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let result: RemoteJobResult =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.results.insert(job_id, result);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT job_id, event_json FROM events ORDER BY id ASC")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (job_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let event: RemoteJobEvent =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.events.entry(job_id).or_default().push(event);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT upload_id, record_json FROM uploads")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (upload_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let rec: UploadRecord =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.uploads.insert(upload_id, rec);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT asset_id, asset_json FROM assets")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (asset_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let asset: RemoteAssetRef =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.assets.insert(asset_id, asset);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT workspace_id, client_request_id, job_id FROM idempotency")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (ws, cr, job_id) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                g.idempotency.insert(IdempotencyKey::new(ws, cr), job_id);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT batch_id, batch_json FROM review_batches")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (_batch_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let batch: RemoteBatch =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.review_batches.push(RemoteReviewBatchSummary {
                    batch_id: batch.batch_id.clone(),
                    name: batch.name.clone(),
                    source: batch.source,
                    item_count: batch.item_count,
                    updated_at: batch.updated_at,
                    cover_asset: batch.cover_asset.clone(),
                });
                g.batches.insert(batch.batch_id.clone(), batch);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT item_id, item_json FROM review_items")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (item_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let item: RemoteReviewItem =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.review_items.insert(item_id, item);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT annotation_id, annotation_json FROM annotations")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (annotation_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let annotation: RemoteAnnotation =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.annotations.insert(annotation_id, annotation);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT result_id, result_json FROM extract_results")
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            for row in rows {
                let (_result_id, json) = row.map_err(|e| StoreError::Internal(e.to_string()))?;
                let summary: RemoteExtractResultSummary =
                    serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
                g.extract_results.push(summary);
            }
        }

        // next_id
        g.next_id = g.jobs.len() as u64 + g.uploads.len() as u64 + g.assets.len() as u64 + 100;
        Ok(())
    }

    fn persist_job(&self, job_id: &str) -> StoreResult<()> {
        let g = self.memory.lock()?;
        let status = g
            .jobs
            .get(job_id)
            .ok_or_else(|| StoreError::NotFound(job_id.into()))?;
        let request = g
            .requests
            .get(job_id)
            .ok_or_else(|| StoreError::Internal("missing request".into()))?;
        let attempt = g.attempts.get(job_id).copied().unwrap_or(0);
        let status_json =
            serde_json::to_string(status).map_err(|e| StoreError::Internal(e.to_string()))?;
        let request_json =
            serde_json::to_string(request).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO jobs(job_id, status_json, request_json, attempt) VALUES (?1,?2,?3,?4)
             ON CONFLICT(job_id) DO UPDATE SET status_json=excluded.status_json,
               request_json=excluded.request_json, attempt=excluded.attempt",
            rusqlite::params![job_id, status_json, request_json, attempt],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        if let Some(cr) = &request.client_request_id {
            let ws = request
                .workspace_id
                .clone()
                .unwrap_or_else(|| "default".into());
            conn.execute(
                "INSERT OR REPLACE INTO idempotency(workspace_id, client_request_id, job_id)
                 VALUES (?1,?2,?3)",
                rusqlite::params![ws, cr, job_id],
            )
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    fn persist_result(&self, job_id: &str) -> StoreResult<()> {
        let g = self.memory.lock()?;
        let result = g
            .results
            .get(job_id)
            .ok_or_else(|| StoreError::NotFound(job_id.into()))?;
        let json =
            serde_json::to_string(result).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO results(job_id, result_json) VALUES (?1,?2)
             ON CONFLICT(job_id) DO UPDATE SET result_json=excluded.result_json",
            rusqlite::params![job_id, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_event(&self, event: &RemoteJobEvent) -> StoreResult<()> {
        let json = serde_json::to_string(event).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO events(job_id, ts, event_json) VALUES (?1,?2,?3)",
            rusqlite::params![event.job_id, event.ts as i64, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_upload(&self, upload_id: &str) -> StoreResult<()> {
        let g = self.memory.lock()?;
        let rec = g
            .uploads
            .get(upload_id)
            .ok_or_else(|| StoreError::NotFound(upload_id.into()))?;
        let json = serde_json::to_string(rec).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO uploads(upload_id, record_json) VALUES (?1,?2)
             ON CONFLICT(upload_id) DO UPDATE SET record_json=excluded.record_json",
            rusqlite::params![upload_id, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn delete_upload_row(&self, upload_id: &str) -> StoreResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "DELETE FROM uploads WHERE upload_id=?1",
            rusqlite::params![upload_id],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_asset(&self, asset_id: &str) -> StoreResult<()> {
        let g = self.memory.lock()?;
        let asset = g
            .assets
            .get(asset_id)
            .ok_or_else(|| StoreError::NotFound(asset_id.into()))?;
        let json = serde_json::to_string(asset).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO assets(asset_id, asset_json) VALUES (?1,?2)
             ON CONFLICT(asset_id) DO UPDATE SET asset_json=excluded.asset_json",
            rusqlite::params![asset_id, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_dead_letter(&self, record: &DeadLetterRecord) -> StoreResult<()> {
        let json =
            serde_json::to_string(record).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO dead_letters(record_json) VALUES (?1)",
            rusqlite::params![json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_batch(&self, batch_id: &str) -> StoreResult<()> {
        let batch = self.memory.get_batch(batch_id)?;
        let json =
            serde_json::to_string(&batch).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO review_batches(batch_id, batch_json) VALUES (?1,?2)
             ON CONFLICT(batch_id) DO UPDATE SET batch_json=excluded.batch_json",
            rusqlite::params![batch_id, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_item(&self, item_id: &str) -> StoreResult<()> {
        let item = self.memory.get_review_item(item_id)?;
        let json = serde_json::to_string(&item).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO review_items(item_id, batch_id, item_json) VALUES (?1,?2,?3)
             ON CONFLICT(item_id) DO UPDATE SET batch_id=excluded.batch_id, item_json=excluded.item_json",
            rusqlite::params![item_id, item.batch_id, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_annotation(&self, annotation_id: &str) -> StoreResult<()> {
        let annotation = self.memory.get_annotation(annotation_id)?;
        let json =
            serde_json::to_string(&annotation).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO annotations(annotation_id, item_id, annotation_json) VALUES (?1,?2,?3)
             ON CONFLICT(annotation_id) DO UPDATE SET item_id=excluded.item_id, annotation_json=excluded.annotation_json",
            rusqlite::params![annotation_id, annotation.item_id, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn persist_extract(&self, result_id: &str) -> StoreResult<()> {
        let page = self.memory.list_extract_results(RemotePageQuery {
            limit: 10_000,
            ..Default::default()
        })?;
        let summary = page
            .items
            .into_iter()
            .find(|r| r.result_id == result_id)
            .ok_or_else(|| StoreError::NotFound(result_id.into()))?;
        let json =
            serde_json::to_string(&summary).map_err(|e| StoreError::Internal(e.to_string()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO extract_results(result_id, result_json) VALUES (?1,?2)
             ON CONFLICT(result_id) DO UPDATE SET result_json=excluded.result_json",
            rusqlite::params![result_id, json],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// UploadRecord needs Serialize for SQLite — add derives via serde in this module.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadRecordSerde {
    init: RemoteUploadInitRequest,
    object_key: String,
    bytes_received: Option<u64>,
    received_checksum: Option<String>,
    completed: bool,
}

impl Serialize for UploadRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        UploadRecordSerde {
            init: self.init.clone(),
            object_key: self.object_key.clone(),
            bytes_received: self.bytes_received,
            received_checksum: self.received_checksum.clone(),
            completed: self.completed,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UploadRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = UploadRecordSerde::deserialize(deserializer)?;
        Ok(UploadRecord {
            init: s.init,
            object_key: s.object_key,
            bytes_received: s.bytes_received,
            received_checksum: s.received_checksum,
            completed: s.completed,
        })
    }
}

impl JobStore for SqliteJobStore {
    fn create_job(&self, request: RemoteJobRequest) -> StoreResult<RemoteJobStatus> {
        let status = self.memory.create_job(request)?;
        self.persist_job(&status.job_id)?;
        Ok(status)
    }

    fn get_job(&self, job_id: &str) -> StoreResult<RemoteJobStatus> {
        self.memory.get_job(job_id)
    }

    fn get_job_record(&self, job_id: &str) -> StoreResult<RemoteJobRecord> {
        self.memory.get_job_record(job_id)
    }

    fn list_jobs(&self, limit: usize) -> StoreResult<Vec<RemoteJobSummary>> {
        self.memory.list_jobs(limit)
    }

    fn update_job(&self, status: RemoteJobStatus) -> StoreResult<()> {
        let job_id = status.job_id.clone();
        self.memory.update_job(status)?;
        self.persist_job(&job_id)?;
        // also persist the phase-change event that memory just appended
        if let Ok(events) = self.memory.list_events(&job_id, None) {
            if let Some(last) = events.last() {
                let _ = self.persist_event(last);
            }
        }
        Ok(())
    }

    fn set_result(&self, result: RemoteJobResult) -> StoreResult<()> {
        let job_id = result.job_id.clone();
        self.memory.set_result(result)?;
        self.persist_result(&job_id)
    }

    fn get_result(&self, job_id: &str) -> StoreResult<RemoteJobResult> {
        self.memory.get_result(job_id)
    }

    fn append_event(&self, event: RemoteJobEvent) -> StoreResult<()> {
        self.persist_event(&event)?;
        self.memory.append_event(event)
    }

    fn list_events(&self, job_id: &str, after_ts: Option<u64>) -> StoreResult<Vec<RemoteJobEvent>> {
        self.memory.list_events(job_id, after_ts)
    }

    fn init_upload(
        &self,
        request: RemoteUploadInitRequest,
        public_base: &str,
    ) -> StoreResult<RemoteUploadSession> {
        let session = self.memory.init_upload(request, public_base)?;
        self.persist_upload(&session.upload_id)?;
        Ok(session)
    }

    fn get_upload(&self, upload_id: &str) -> StoreResult<UploadRecord> {
        self.memory.get_upload(upload_id)
    }

    fn mark_upload_bytes(
        &self,
        upload_id: &str,
        size: u64,
        checksum: Option<String>,
    ) -> StoreResult<()> {
        self.memory.mark_upload_bytes(upload_id, size, checksum)?;
        self.persist_upload(upload_id)
    }

    fn complete_upload(
        &self,
        request: RemoteUploadCompleteRequest,
        public_base: &str,
    ) -> StoreResult<RemoteUploadCompleteResponse> {
        let resp = self.memory.complete_upload(request.clone(), public_base)?;
        self.persist_upload(&request.upload_id)?;
        self.persist_asset(&resp.asset.id)?;
        Ok(resp)
    }

    fn abort_upload(&self, upload_id: &str) -> StoreResult<()> {
        self.memory.abort_upload(upload_id)?;
        self.delete_upload_row(upload_id)
    }

    fn register_asset(&self, asset: RemoteAssetRef) -> StoreResult<()> {
        let id = asset.id.clone();
        self.memory.register_asset(asset)?;
        self.persist_asset(&id)
    }

    fn get_asset(&self, asset_id: &str) -> StoreResult<RemoteAssetRef> {
        self.memory.get_asset(asset_id)
    }

    fn download_credential(
        &self,
        asset_id: &str,
        public_base: &str,
    ) -> StoreResult<RemoteDownloadCredential> {
        self.memory.download_credential(asset_id, public_base)
    }

    fn list_assets(&self, query: RemotePageQuery) -> StoreResult<RemotePage<RemoteAssetListItem>> {
        self.memory.list_assets(query)
    }

    fn list_review_batches(
        &self,
        query: RemotePageQuery,
    ) -> StoreResult<RemotePage<RemoteReviewBatchSummary>> {
        self.memory.list_review_batches(query)
    }

    fn list_extract_results(
        &self,
        query: RemotePageQuery,
    ) -> StoreResult<RemotePage<RemoteExtractResultSummary>> {
        self.memory.list_extract_results(query)
    }

    fn put_dead_letter(&self, record: DeadLetterRecord) -> StoreResult<()> {
        self.persist_dead_letter(&record)?;
        self.memory.put_dead_letter(record)
    }

    fn list_dead_letters(&self) -> StoreResult<Vec<DeadLetterRecord>> {
        self.memory.list_dead_letters()
    }

    fn create_batch(&self, request: CreateRemoteBatchRequest) -> StoreResult<RemoteBatch> {
        let batch = self.memory.create_batch(request)?;
        self.persist_batch(&batch.batch_id)?;
        for item in self.memory.list_review_items(&batch.batch_id)? {
            self.persist_item(&item.item_id)?;
        }
        Ok(batch)
    }

    fn get_batch(&self, batch_id: &str) -> StoreResult<RemoteBatch> {
        self.memory.get_batch(batch_id)
    }

    fn list_review_items(&self, batch_id: &str) -> StoreResult<Vec<RemoteReviewItem>> {
        self.memory.list_review_items(batch_id)
    }

    fn get_review_item(&self, item_id: &str) -> StoreResult<RemoteReviewItem> {
        self.memory.get_review_item(item_id)
    }

    fn update_review_item(
        &self,
        item_id: &str,
        request: UpdateRemoteReviewItemRequest,
    ) -> StoreResult<RemoteReviewItem> {
        let item = self.memory.update_review_item(item_id, request)?;
        self.persist_item(&item.item_id)?;
        self.persist_batch(&item.batch_id)?;
        Ok(item)
    }

    fn update_review_item_assets(
        &self,
        item_id: &str,
        thumb_asset: Option<RemoteAssetRef>,
        preview_asset: Option<RemoteAssetRef>,
        duration_ms: Option<u64>,
        dimensions: Option<(u32, u32)>,
    ) -> StoreResult<RemoteReviewItem> {
        let item = self.memory.update_review_item_assets(
            item_id,
            thumb_asset,
            preview_asset,
            duration_ms,
            dimensions,
        )?;
        self.persist_item(&item.item_id)?;
        self.persist_batch(&item.batch_id)?;
        Ok(item)
    }

    fn upsert_annotation(&self, annotation: RemoteAnnotation) -> StoreResult<RemoteAnnotation> {
        let annotation = self.memory.upsert_annotation(annotation)?;
        self.persist_annotation(&annotation.annotation_id)?;
        Ok(annotation)
    }

    fn list_annotations(&self, item_id: &str) -> StoreResult<Vec<RemoteAnnotation>> {
        self.memory.list_annotations(item_id)
    }

    fn get_annotation(&self, annotation_id: &str) -> StoreResult<RemoteAnnotation> {
        self.memory.get_annotation(annotation_id)
    }

    fn upsert_extract_result(
        &self,
        summary: RemoteExtractResultSummary,
    ) -> StoreResult<RemoteExtractResultSummary> {
        let result_id = summary.result_id.clone();
        let out = self.memory.upsert_extract_result(summary)?;
        self.persist_extract(&result_id)?;
        Ok(out)
    }

    fn append_audit(
        &self,
        workspace_id: Option<&str>,
        actor: Option<&str>,
        action: &str,
        detail: Option<&str>,
    ) -> StoreResult<()> {
        let ts = now_unix() as i64;
        let conn = self
            .conn
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO audit_log(ts, workspace_id, actor, action, detail)
             VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![ts, workspace_id, actor, action, detail],
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::types::RemoteJobSource;
    use tempfile::tempdir;

    #[test]
    fn idempotent_create_returns_same_job() {
        let store = InMemoryJobStore::new();
        let req = RemoteJobRequest {
            source: RemoteJobSource::Convert,
            workspace_id: Some("ws".into()),
            client_request_id: Some("cr-1".into()),
            ..RemoteJobRequest::default()
        };
        let a = store.create_job(req.clone()).unwrap();
        let b = store.create_job(req).unwrap();
        assert_eq!(a.job_id, b.job_id);
        let rec = store.get_job_record(&a.job_id).unwrap();
        assert_eq!(rec.request.source, RemoteJobSource::Convert);
    }

    #[test]
    fn sqlite_persists_across_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("meta.sqlite");
        {
            let store = SqliteJobStore::open(&path).unwrap();
            let status = store
                .create_job(RemoteJobRequest {
                    client_request_id: Some("x".into()),
                    ..RemoteJobRequest::default()
                })
                .unwrap();
            assert!(!status.job_id.is_empty());
        }
        let store2 = SqliteJobStore::open(&path).unwrap();
        let jobs = store2.list_jobs(10).unwrap();
        assert_eq!(jobs.len(), 1);
    }
}
