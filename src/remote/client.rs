//! RemoteClient trait、Disabled / Mock / HTTP 实现。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::remote::catalog::{
    RemoteAssetListItem, RemoteExtractResultSummary, RemotePage, RemotePageQuery,
    RemoteReviewBatchSummary,
};
use crate::remote::config::RemoteConfig;
use crate::remote::error::{RemoteError, RemoteResult};
use crate::remote::events::RemoteJobEvent;
use crate::remote::http::HttpRemoteClient;
use crate::remote::models::{
    CreateRemoteBatchRequest, RemoteAnnotation, RemoteBatch, RemoteBatchKind, RemoteReviewItem,
    RemoteReviewItemStatus, UpdateRemoteReviewItemRequest,
};
use crate::remote::types::{
    now_unix, RemoteAssetRef, RemoteHealth, RemoteJobPhase, RemoteJobRequest, RemoteJobResult,
    RemoteJobSource, RemoteJobStatus, RemoteJobSummary, REMOTE_SCHEMA_VERSION,
};
use crate::remote::upload::{
    RemoteDownloadCredential, RemoteUploadAbortRequest, RemoteUploadCompleteRequest,
    RemoteUploadCompleteResponse, RemoteUploadInitRequest, RemoteUploadSession,
};

/// 远端 API 客户端抽象：屏蔽 HTTP / 认证 / 重试细节。
pub trait RemoteClient: Send + Sync {
    fn health(&self) -> RemoteResult<RemoteHealth>;
    fn submit_job(&self, request: RemoteJobRequest) -> RemoteResult<RemoteJobStatus>;
    fn get_job(&self, job_id: &str) -> RemoteResult<RemoteJobStatus>;
    fn list_jobs(&self, limit: usize) -> RemoteResult<Vec<RemoteJobSummary>>;
    fn get_result(&self, job_id: &str) -> RemoteResult<RemoteJobResult>;
    fn cancel_job(&self, job_id: &str) -> RemoteResult<RemoteJobStatus>;

    /// 初始化上传会话（预签名 / multipart / tus）。
    fn init_upload(&self, request: RemoteUploadInitRequest) -> RemoteResult<RemoteUploadSession> {
        let _ = request;
        Err(RemoteError::Other(
            "upload data plane not supported by this client".into(),
        ))
    }

    fn complete_upload(
        &self,
        request: RemoteUploadCompleteRequest,
    ) -> RemoteResult<RemoteUploadCompleteResponse> {
        let _ = request;
        Err(RemoteError::Other(
            "upload data plane not supported by this client".into(),
        ))
    }

    fn abort_upload(&self, request: RemoteUploadAbortRequest) -> RemoteResult<()> {
        let _ = request;
        Err(RemoteError::Other(
            "upload data plane not supported by this client".into(),
        ))
    }

    fn artifact_download_url(&self, asset_id: &str) -> RemoteResult<RemoteDownloadCredential> {
        let _ = asset_id;
        Err(RemoteError::Other(
            "artifact download not supported by this client".into(),
        ))
    }

    /// 向上传会话 PUT 原始字节（本地服务器数据面）。
    fn upload_bytes(&self, upload_url: &str, bytes: &[u8]) -> RemoteResult<()> {
        let _ = (upload_url, bytes);
        Err(RemoteError::Other(
            "upload bytes not supported by this client".into(),
        ))
    }

    /// 下载 artifact 内容字节。
    fn download_bytes(&self, download_url: &str) -> RemoteResult<Vec<u8>> {
        let _ = download_url;
        Err(RemoteError::Other(
            "download bytes not supported by this client".into(),
        ))
    }

    /// 轮询兼容的事件拉取（SSE 客户端可后续替换）。
    fn list_job_events(
        &self,
        job_id: &str,
        after_ts: Option<u64>,
    ) -> RemoteResult<Vec<RemoteJobEvent>> {
        let _ = (job_id, after_ts);
        Err(RemoteError::Other(
            "job events not supported by this client".into(),
        ))
    }

    fn list_assets(&self, query: RemotePageQuery) -> RemoteResult<RemotePage<RemoteAssetListItem>> {
        let _ = query;
        Err(RemoteError::Other(
            "asset catalog not supported by this client".into(),
        ))
    }

    fn list_review_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        let _ = query;
        Err(RemoteError::Other(
            "review catalog not supported by this client".into(),
        ))
    }

    fn list_video_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        let mut page = self.list_review_batches(query)?;
        page.items
            .retain(|item| item.source == RemoteJobSource::VideoReview);
        page.total = page.items.len();
        Ok(page)
    }

    fn create_batch(&self, request: CreateRemoteBatchRequest) -> RemoteResult<RemoteBatch> {
        let _ = request;
        Err(RemoteError::Other(
            "review batch mutation not supported by this client".into(),
        ))
    }

    fn get_batch(&self, batch_id: &str) -> RemoteResult<RemoteBatch> {
        let _ = batch_id;
        Err(RemoteError::Other(
            "review batch detail not supported by this client".into(),
        ))
    }

    fn list_review_items(&self, batch_id: &str) -> RemoteResult<Vec<RemoteReviewItem>> {
        let _ = batch_id;
        Err(RemoteError::Other(
            "review items not supported by this client".into(),
        ))
    }

    fn update_review_item(
        &self,
        item_id: &str,
        request: UpdateRemoteReviewItemRequest,
    ) -> RemoteResult<RemoteReviewItem> {
        let _ = (item_id, request);
        Err(RemoteError::Other(
            "review item mutation not supported by this client".into(),
        ))
    }

    fn upsert_annotation(&self, annotation: RemoteAnnotation) -> RemoteResult<RemoteAnnotation> {
        let _ = annotation;
        Err(RemoteError::Other(
            "review annotations not supported by this client".into(),
        ))
    }

    fn list_annotations(&self, item_id: &str) -> RemoteResult<Vec<RemoteAnnotation>> {
        let _ = item_id;
        Err(RemoteError::Other(
            "review annotations not supported by this client".into(),
        ))
    }

    fn list_extract_results(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteExtractResultSummary>> {
        let _ = query;
        Err(RemoteError::Other(
            "extract catalog not supported by this client".into(),
        ))
    }
}

/// 未启用远端时的空实现。
#[derive(Debug, Default, Clone)]
pub struct DisabledRemoteClient;

impl RemoteClient for DisabledRemoteClient {
    fn health(&self) -> RemoteResult<RemoteHealth> {
        Ok(RemoteHealth {
            ok: false,
            message: "远端未启用（remote.enabled=false）".into(),
            server_version: None,
            schema_version: Some(REMOTE_SCHEMA_VERSION),
        })
    }

    fn submit_job(&self, _request: RemoteJobRequest) -> RemoteResult<RemoteJobStatus> {
        Err(RemoteError::Disabled)
    }

    fn get_job(&self, _job_id: &str) -> RemoteResult<RemoteJobStatus> {
        Err(RemoteError::Disabled)
    }

    fn list_jobs(&self, _limit: usize) -> RemoteResult<Vec<RemoteJobSummary>> {
        Err(RemoteError::Disabled)
    }

    fn get_result(&self, _job_id: &str) -> RemoteResult<RemoteJobResult> {
        Err(RemoteError::Disabled)
    }

    fn cancel_job(&self, _job_id: &str) -> RemoteResult<RemoteJobStatus> {
        Err(RemoteError::Disabled)
    }
}

/// 内存 Mock：用于测试与本地联调，不依赖真实服务器。
#[derive(Debug, Clone)]
pub struct MockRemoteClient {
    inner: Arc<Mutex<MockState>>,
}

#[derive(Debug, Default)]
struct MockState {
    jobs: HashMap<String, RemoteJobStatus>,
    results: HashMap<String, RemoteJobResult>,
    uploads: HashMap<String, RemoteUploadInitRequest>,
    upload_bytes: HashMap<String, Vec<u8>>,
    assets: HashMap<String, RemoteAssetRef>,
    asset_bytes: HashMap<String, Vec<u8>>,
    batches: HashMap<String, RemoteBatch>,
    review_items: HashMap<String, RemoteReviewItem>,
    annotations: HashMap<String, RemoteAnnotation>,
    events: HashMap<String, Vec<RemoteJobEvent>>,
    next_id: u64,
    fail_submit: bool,
    offline: bool,
}

impl Default for MockRemoteClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRemoteClient {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MockState::default())),
        }
    }

    pub fn set_fail_submit(&self, fail: bool) {
        if let Ok(mut g) = self.inner.lock() {
            g.fail_submit = fail;
        }
    }

    pub fn set_offline(&self, offline: bool) {
        if let Ok(mut g) = self.inner.lock() {
            g.offline = offline;
        }
    }

    /// 推进任务到指定阶段（测试辅助）。
    pub fn advance(&self, job_id: &str, phase: RemoteJobPhase) -> RemoteResult<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let (processed, error_summary, source) = {
            let job = g
                .jobs
                .get_mut(job_id)
                .ok_or_else(|| RemoteError::JobNotFound(job_id.to_string()))?;
            job.phase = phase;
            job.updated_at = now_unix();
            let processed = if phase == RemoteJobPhase::Succeeded {
                job.progress = Some(1.0);
                let processed = job.total.max(1);
                job.processed = processed;
                processed
            } else {
                job.processed
            };
            let error_summary = if phase == RemoteJobPhase::Failed {
                job.error_summary = Some("mock failure".into());
                Some("mock failure".to_string())
            } else {
                None
            };
            (processed, error_summary, job.source)
        };

        let event = RemoteJobEvent::phase_change(job_id, source, phase, now_unix());
        g.events.entry(job_id.to_string()).or_default().push(event);

        if phase == RemoteJobPhase::Succeeded {
            g.results.insert(
                job_id.to_string(),
                RemoteJobResult {
                    schema_version: REMOTE_SCHEMA_VERSION,
                    job_id: job_id.to_string(),
                    phase,
                    successes: processed,
                    failures: 0,
                    artifacts: Vec::new(),
                    error_summary: None,
                    updated_at: now_unix(),
                },
            );
        }
        if phase == RemoteJobPhase::Failed {
            g.results.insert(
                job_id.to_string(),
                RemoteJobResult {
                    schema_version: REMOTE_SCHEMA_VERSION,
                    job_id: job_id.to_string(),
                    phase,
                    successes: 0,
                    failures: 1,
                    artifacts: Vec::new(),
                    error_summary,
                    updated_at: now_unix(),
                },
            );
        }
        Ok(())
    }
}

impl RemoteClient for MockRemoteClient {
    fn health(&self) -> RemoteResult<RemoteHealth> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Ok(RemoteHealth {
                ok: false,
                message: "mock offline".into(),
                server_version: Some("mock".into()),
                schema_version: Some(REMOTE_SCHEMA_VERSION),
            });
        }
        Ok(RemoteHealth {
            ok: true,
            message: "mock ok".into(),
            server_version: Some("mock-0.1".into()),
            schema_version: Some(REMOTE_SCHEMA_VERSION),
        })
    }

    fn submit_job(&self, request: RemoteJobRequest) -> RemoteResult<RemoteJobStatus> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        if g.fail_submit {
            return Err(RemoteError::Request("mock submit failed".into()));
        }
        g.next_id += 1;
        let job_id = format!("mock-{}", g.next_id);
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
        g.jobs.insert(job_id, status.clone());
        Ok(status)
    }

    fn get_job(&self, job_id: &str) -> RemoteResult<RemoteJobStatus> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        g.jobs
            .get(job_id)
            .cloned()
            .ok_or_else(|| RemoteError::JobNotFound(job_id.to_string()))
    }

    fn list_jobs(&self, limit: usize) -> RemoteResult<Vec<RemoteJobSummary>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        let mut jobs: Vec<_> = g.jobs.values().map(RemoteJobSummary::from).collect();
        jobs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        jobs.truncate(limit.max(1));
        Ok(jobs)
    }

    fn get_result(&self, job_id: &str) -> RemoteResult<RemoteJobResult> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        g.results
            .get(job_id)
            .cloned()
            .ok_or_else(|| RemoteError::JobNotFound(job_id.to_string()))
    }

    fn cancel_job(&self, job_id: &str) -> RemoteResult<RemoteJobStatus> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        let (source, was_terminal) = {
            let job = g
                .jobs
                .get_mut(job_id)
                .ok_or_else(|| RemoteError::JobNotFound(job_id.to_string()))?;
            let was_terminal = job.phase.is_terminal();
            if !was_terminal {
                job.phase = RemoteJobPhase::Cancelled;
                job.updated_at = now_unix();
            }
            (job.source, was_terminal)
        };
        if !was_terminal {
            let event =
                RemoteJobEvent::phase_change(job_id, source, RemoteJobPhase::Cancelled, now_unix());
            g.events.entry(job_id.to_string()).or_default().push(event);
        }
        g.jobs
            .get(job_id)
            .cloned()
            .ok_or_else(|| RemoteError::JobNotFound(job_id.to_string()))
    }

    fn init_upload(&self, request: RemoteUploadInitRequest) -> RemoteResult<RemoteUploadSession> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        g.next_id += 1;
        let upload_id = format!("upload-{}", g.next_id);
        g.uploads.insert(upload_id.clone(), request.clone());
        Ok(RemoteUploadSession::single_put(
            &upload_id,
            format!("mock://uploads/{upload_id}/bytes"),
            request.size.unwrap_or(8 * 1024 * 1024),
            3600,
        ))
    }

    fn complete_upload(
        &self,
        request: RemoteUploadCompleteRequest,
    ) -> RemoteResult<RemoteUploadCompleteResponse> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        let init = g
            .uploads
            .remove(&request.upload_id)
            .ok_or_else(|| RemoteError::JobNotFound(request.upload_id.clone()))?;
        let bytes = g
            .upload_bytes
            .remove(&request.upload_id)
            .ok_or_else(|| RemoteError::Request("upload bytes missing".into()))?;
        let asset_id = format!("asset-{}", request.upload_id);
        let asset = RemoteAssetRef {
            id: asset_id.clone(),
            name: init.file_name,
            mime: init.mime,
            size: Some(bytes.len() as u64),
            checksum: request.checksum.or(init.checksum),
            download_url: Some(format!("mock://artifacts/{asset_id}/content")),
        };
        g.asset_bytes.insert(asset_id, bytes);
        g.assets.insert(asset.id.clone(), asset.clone());
        Ok(RemoteUploadCompleteResponse {
            schema_version: REMOTE_SCHEMA_VERSION,
            asset,
        })
    }

    fn abort_upload(&self, request: RemoteUploadAbortRequest) -> RemoteResult<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        g.uploads.remove(&request.upload_id);
        g.upload_bytes.remove(&request.upload_id);
        Ok(())
    }

    fn artifact_download_url(&self, asset_id: &str) -> RemoteResult<RemoteDownloadCredential> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let asset = g
            .assets
            .get(asset_id)
            .ok_or_else(|| RemoteError::JobNotFound(asset_id.to_string()))?;
        Ok(RemoteDownloadCredential {
            schema_version: REMOTE_SCHEMA_VERSION,
            asset_id: asset.id.clone(),
            download_url: asset
                .download_url
                .clone()
                .unwrap_or_else(|| format!("mock://artifacts/{asset_id}/content")),
            expires_at: now_unix() + 3600,
            checksum: asset.checksum.clone(),
        })
    }

    fn upload_bytes(&self, upload_url: &str, bytes: &[u8]) -> RemoteResult<()> {
        let upload_id = upload_url
            .trim_start_matches("mock://uploads/")
            .trim_end_matches("/bytes")
            .to_string();
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if !g.uploads.contains_key(&upload_id) {
            return Err(RemoteError::JobNotFound(upload_id));
        }
        g.upload_bytes.insert(upload_id, bytes.to_vec());
        Ok(())
    }

    fn download_bytes(&self, download_url: &str) -> RemoteResult<Vec<u8>> {
        let asset_id = download_url
            .trim_start_matches("mock://artifacts/")
            .trim_end_matches("/content")
            .to_string();
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        g.asset_bytes
            .get(&asset_id)
            .cloned()
            .ok_or_else(|| RemoteError::JobNotFound(asset_id))
    }

    fn list_job_events(
        &self,
        job_id: &str,
        after_ts: Option<u64>,
    ) -> RemoteResult<Vec<RemoteJobEvent>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let events = g.events.get(job_id).cloned().unwrap_or_default();
        Ok(events
            .into_iter()
            .filter(|e| after_ts.map(|ts| e.ts > ts).unwrap_or(true))
            .collect())
    }

    fn list_assets(&self, query: RemotePageQuery) -> RemoteResult<RemotePage<RemoteAssetListItem>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
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
        let page: Vec<_> = items
            .into_iter()
            .skip(query.offset)
            .take(query.limit.max(1))
            .collect();
        Ok(RemotePage::new(page, total))
    }

    fn list_review_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let mut items: Vec<_> = g
            .batches
            .values()
            .filter(|b| {
                query
                    .workspace_id
                    .as_ref()
                    .map(|w| b.workspace_id.as_deref() == Some(w.as_str()))
                    .unwrap_or(true)
            })
            .map(|b| RemoteReviewBatchSummary {
                batch_id: b.batch_id.clone(),
                name: b.name.clone(),
                source: b.source,
                item_count: b.item_count,
                updated_at: b.updated_at,
                cover_asset: b.cover_asset.clone(),
            })
            .collect();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let total = items.len();
        let page = items
            .into_iter()
            .skip(query.offset)
            .take(query.limit.max(1))
            .collect();
        Ok(RemotePage::new(page, total))
    }

    fn list_video_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let mut items: Vec<_> = g
            .batches
            .values()
            .filter(|b| b.source == RemoteJobSource::VideoReview)
            .filter(|b| {
                query
                    .workspace_id
                    .as_ref()
                    .map(|w| b.workspace_id.as_deref() == Some(w.as_str()))
                    .unwrap_or(true)
            })
            .map(|b| RemoteReviewBatchSummary {
                batch_id: b.batch_id.clone(),
                name: b.name.clone(),
                source: b.source,
                item_count: b.item_count,
                updated_at: b.updated_at,
                cover_asset: b.cover_asset.clone(),
            })
            .collect();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let total = items.len();
        let page = items
            .into_iter()
            .skip(query.offset)
            .take(query.limit.max(1))
            .collect();
        Ok(RemotePage::new(page, total))
    }

    fn create_batch(&self, request: CreateRemoteBatchRequest) -> RemoteResult<RemoteBatch> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        if g.offline {
            return Err(RemoteError::Request("mock offline".into()));
        }
        g.next_id += 1;
        let batch_id = format!("batch-{}", g.next_id);
        let now = now_unix();
        let source = match request.kind {
            RemoteBatchKind::Image => RemoteJobSource::Review,
            RemoteBatchKind::Video => RemoteJobSource::VideoReview,
        };
        let mut cover_asset = None;
        let mut item_count = 0usize;
        for asset_id in &request.asset_ids {
            let asset = g
                .assets
                .get(asset_id)
                .cloned()
                .ok_or_else(|| RemoteError::JobNotFound(format!("asset {asset_id}")))?;
            if cover_asset.is_none() {
                cover_asset = Some(asset.clone());
            }
            g.next_id += 1;
            let item_id = format!("item-{}", g.next_id);
            g.review_items.insert(
                item_id.clone(),
                RemoteReviewItem {
                    schema_version: REMOTE_SCHEMA_VERSION,
                    item_id,
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
                },
            );
            item_count += 1;
        }
        let batch = RemoteBatch {
            schema_version: REMOTE_SCHEMA_VERSION,
            batch_id: batch_id.clone(),
            name: request.name,
            kind: request.kind,
            source,
            workspace_id: request.workspace_id,
            item_count,
            created_at: now,
            updated_at: now,
            cover_asset,
            status_summary: Some("pending".into()),
        };
        g.batches.insert(batch_id, batch.clone());
        Ok(batch)
    }

    fn get_batch(&self, batch_id: &str) -> RemoteResult<RemoteBatch> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        g.batches
            .get(batch_id)
            .cloned()
            .ok_or_else(|| RemoteError::JobNotFound(batch_id.to_string()))
    }

    fn list_review_items(&self, batch_id: &str) -> RemoteResult<Vec<RemoteReviewItem>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let mut items: Vec<_> = g
            .review_items
            .values()
            .filter(|item| item.batch_id == batch_id)
            .cloned()
            .collect();
        items.sort_by(|a, b| a.item_id.cmp(&b.item_id));
        Ok(items)
    }

    fn update_review_item(
        &self,
        item_id: &str,
        request: UpdateRemoteReviewItemRequest,
    ) -> RemoteResult<RemoteReviewItem> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let out = {
            let item = g
                .review_items
                .get_mut(item_id)
                .ok_or_else(|| RemoteError::JobNotFound(item_id.to_string()))?;
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
            item.clone()
        };
        if let Some(batch) = g.batches.get_mut(&out.batch_id) {
            batch.updated_at = out.updated_at;
        }
        Ok(out)
    }

    fn upsert_annotation(&self, annotation: RemoteAnnotation) -> RemoteResult<RemoteAnnotation> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let mut annotation = annotation;
        if annotation.annotation_id.is_empty() {
            g.next_id += 1;
            annotation.annotation_id = format!("ann-{}", g.next_id);
        }
        if annotation.schema_version == 0 {
            annotation.schema_version = REMOTE_SCHEMA_VERSION;
        }
        if annotation.created_at == 0 {
            annotation.created_at = now_unix();
        }
        g.annotations
            .insert(annotation.annotation_id.clone(), annotation.clone());
        Ok(annotation)
    }

    fn list_annotations(&self, item_id: &str) -> RemoteResult<Vec<RemoteAnnotation>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| RemoteError::Other(e.to_string()))?;
        let mut items: Vec<_> = g
            .annotations
            .values()
            .filter(|annotation| annotation.item_id == item_id)
            .cloned()
            .collect();
        items.sort_by(|a, b| a.annotation_id.cmp(&b.annotation_id));
        Ok(items)
    }

    fn list_extract_results(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteExtractResultSummary>> {
        let _ = query;
        Ok(RemotePage::new(Vec::new(), 0))
    }
}

/// 根据配置构造客户端：未启用 → Disabled；已配置 → HTTP。
pub fn build_client(config: &RemoteConfig) -> Arc<dyn RemoteClient> {
    if !config.enabled {
        return Arc::new(DisabledRemoteClient);
    }
    if !config.is_configured() {
        return Arc::new(DisabledRemoteClient);
    }
    match HttpRemoteClient::try_new(config) {
        Ok(client) => Arc::new(client),
        Err(e) => {
            tracing::warn!(error = %e, "failed to build HTTP remote client; using disabled");
            Arc::new(DisabledRemoteClient)
        }
    }
}

/// 尝试构造 HTTP 客户端；失败时返回错误（供 CLI/GUI 明确提示）。
pub fn try_build_http_client(config: &RemoteConfig) -> RemoteResult<Arc<dyn RemoteClient>> {
    if !config.enabled {
        return Err(RemoteError::Disabled);
    }
    if !config.is_configured() {
        return Err(RemoteError::NotConfigured("缺少 remote.base_url".into()));
    }
    Ok(Arc::new(HttpRemoteClient::try_new(config)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::events::RemoteJobEventKind;
    use crate::remote::types::RemoteJobSource;

    #[test]
    fn disabled_rejects_submit() {
        let client = DisabledRemoteClient;
        let err = client.submit_job(RemoteJobRequest::default()).unwrap_err();
        assert!(matches!(err, RemoteError::Disabled));
    }

    #[test]
    fn mock_submit_and_list() {
        let client = MockRemoteClient::new();
        let status = client
            .submit_job(RemoteJobRequest {
                source: RemoteJobSource::Convert,
                ..RemoteJobRequest::default()
            })
            .unwrap();
        assert_eq!(status.phase, RemoteJobPhase::Queued);
        let list = client.list_jobs(10).unwrap();
        assert_eq!(list.len(), 1);
        client
            .advance(&status.job_id, RemoteJobPhase::Succeeded)
            .unwrap();
        let result = client.get_result(&status.job_id).unwrap();
        assert_eq!(result.phase, RemoteJobPhase::Succeeded);
        let events = client.list_job_events(&status.job_id, None).unwrap();
        assert!(events
            .iter()
            .any(|e| e.kind == RemoteJobEventKind::Succeeded));
    }

    #[test]
    fn mock_offline_surfaces_request_error() {
        let client = MockRemoteClient::new();
        client.set_offline(true);
        assert!(!client.health().unwrap().ok);
        assert!(client.list_jobs(5).is_err());
    }

    #[test]
    fn mock_upload_roundtrip() {
        let client = MockRemoteClient::new();
        let session = client
            .init_upload(RemoteUploadInitRequest {
                file_name: "a.jpg".into(),
                size: Some(100),
                ..RemoteUploadInitRequest::default()
            })
            .unwrap();
        let url = session.parts[0].upload_url.clone();
        client.upload_bytes(&url, b"hello-bytes").unwrap();
        let done = client
            .complete_upload(RemoteUploadCompleteRequest {
                upload_id: session.upload_id,
                checksum: Some("abc".into()),
                ..RemoteUploadCompleteRequest::default()
            })
            .unwrap();
        assert_eq!(done.asset.name, "a.jpg");
        let cred = client.artifact_download_url(&done.asset.id).unwrap();
        let bytes = client.download_bytes(&cred.download_url).unwrap();
        assert_eq!(bytes, b"hello-bytes");
    }
}
