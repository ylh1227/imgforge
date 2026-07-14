//! Postgres 元数据存储：JSON 文档表，与 SQLite 语义对齐。

use std::sync::{Arc, Mutex};

use postgres::{Client, NoTls};

use super::{InMemoryJobStore, JobStore, RemoteJobRecord, StoreError, StoreResult, UploadRecord};
use crate::remote::catalog::{
    RemoteAssetListItem, RemoteExtractResultSummary, RemotePage, RemotePageQuery,
    RemoteReviewBatchSummary,
};
use crate::remote::events::RemoteJobEvent;
use crate::remote::models::{
    CreateRemoteBatchRequest, RemoteAnnotation, RemoteBatch, RemoteReviewItem,
    UpdateRemoteReviewItemRequest,
};
use crate::remote::types::{
    now_unix, RemoteAssetRef, RemoteJobRequest, RemoteJobResult, RemoteJobStatus, RemoteJobSummary,
};
use crate::remote::upload::{
    RemoteDownloadCredential, RemoteUploadCompleteRequest, RemoteUploadCompleteResponse,
    RemoteUploadInitRequest, RemoteUploadSession,
};
use crate::remote::worker_policy::DeadLetterRecord;

/// Postgres 实现：热路径仍用内存镜像，变更同步写入 Postgres。
pub struct PostgresJobStore {
    memory: InMemoryJobStore,
    client: Arc<Mutex<Client>>,
}

impl PostgresJobStore {
    pub fn connect(database_url: &str) -> StoreResult<Self> {
        let mut client = Client::connect(database_url, NoTls)
            .map_err(|e| StoreError::Internal(format!("postgres connect: {e}")))?;
        migrate(&mut client)?;
        let store = Self {
            memory: InMemoryJobStore::new(),
            client: Arc::new(Mutex::new(client)),
        };
        store.hydrate_from_db()?;
        Ok(store)
    }

    fn with_client<R>(&self, f: impl FnOnce(&mut Client) -> StoreResult<R>) -> StoreResult<R> {
        let mut g = self
            .client
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        f(&mut g)
    }

    fn hydrate_from_db(&self) -> StoreResult<()> {
        let rows = self.with_client(|client| {
            let jobs = client
                .query("SELECT status_json, request_json, attempt FROM jobs", &[])
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let mut out = Vec::new();
            for row in jobs {
                out.push((
                    row.get::<_, String>(0),
                    row.get::<_, String>(1),
                    row.get::<_, i32>(2),
                ));
            }
            Ok(out)
        })?;
        for (status_json, request_json, attempt) in rows {
            let status: RemoteJobStatus = serde_json::from_str(&status_json)
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            let request: RemoteJobRequest = serde_json::from_str(&request_json)
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            self.memory.import_job(status, request, attempt as u32)?;
        }

        let results = self.with_client(|client| {
            Ok(client
                .query("SELECT result_json FROM results", &[])
                .map_err(|e| StoreError::Internal(e.to_string()))?
                .into_iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>())
        })?;
        for json in results {
            let result: RemoteJobResult =
                serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
            self.memory.set_result(result)?;
        }

        let assets = self.with_client(|client| {
            Ok(client
                .query("SELECT asset_json FROM assets", &[])
                .map_err(|e| StoreError::Internal(e.to_string()))?
                .into_iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>())
        })?;
        for json in assets {
            let asset: RemoteAssetRef =
                serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
            self.memory.register_asset(asset)?;
        }

        let batches = self.with_client(|client| {
            Ok(client
                .query("SELECT batch_json FROM review_batches", &[])
                .map_err(|e| StoreError::Internal(e.to_string()))?
                .into_iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>())
        })?;
        for json in batches {
            let batch: RemoteBatch =
                serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
            self.memory.import_batch(batch)?;
        }

        let items = self.with_client(|client| {
            Ok(client
                .query("SELECT item_json FROM review_items", &[])
                .map_err(|e| StoreError::Internal(e.to_string()))?
                .into_iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>())
        })?;
        for json in items {
            let item: RemoteReviewItem =
                serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
            self.memory.import_review_item(item)?;
        }

        let anns = self.with_client(|client| {
            Ok(client
                .query("SELECT annotation_json FROM annotations", &[])
                .map_err(|e| StoreError::Internal(e.to_string()))?
                .into_iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>())
        })?;
        for json in anns {
            let ann: RemoteAnnotation =
                serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
            self.memory.import_annotation(ann)?;
        }

        let extracts = self.with_client(|client| {
            Ok(client
                .query("SELECT result_json FROM extract_results", &[])
                .map_err(|e| StoreError::Internal(e.to_string()))?
                .into_iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>())
        })?;
        for json in extracts {
            let summary: RemoteExtractResultSummary =
                serde_json::from_str(&json).map_err(|e| StoreError::Internal(e.to_string()))?;
            self.memory.import_extract_result(summary)?;
        }

        Ok(())
    }

    fn persist_job(&self, job_id: &str) -> StoreResult<()> {
        let record = self.memory.get_job_record(job_id)?;
        let status_json = serde_json::to_string(&record.status)
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        let request_json = serde_json::to_string(&record.request)
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO jobs(job_id, status_json, request_json, attempt)
                     VALUES ($1,$2,$3,$4)
                     ON CONFLICT (job_id) DO UPDATE SET
                       status_json=EXCLUDED.status_json,
                       request_json=EXCLUDED.request_json,
                       attempt=EXCLUDED.attempt",
                    &[
                        &job_id,
                        &status_json,
                        &request_json,
                        &(record.attempt as i32),
                    ],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
    }

    fn persist_result(&self, job_id: &str) -> StoreResult<()> {
        let result = self.memory.get_result(job_id)?;
        let json =
            serde_json::to_string(&result).map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO results(job_id, result_json) VALUES ($1,$2)
                     ON CONFLICT (job_id) DO UPDATE SET result_json=EXCLUDED.result_json",
                    &[&job_id, &json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
    }

    fn persist_asset(&self, asset_id: &str) -> StoreResult<()> {
        let asset = self.memory.get_asset(asset_id)?;
        let json =
            serde_json::to_string(&asset).map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO assets(asset_id, asset_json) VALUES ($1,$2)
                     ON CONFLICT (asset_id) DO UPDATE SET asset_json=EXCLUDED.asset_json",
                    &[&asset_id, &json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
    }

    fn persist_batch(&self, batch_id: &str) -> StoreResult<()> {
        let batch = self.memory.get_batch(batch_id)?;
        let json =
            serde_json::to_string(&batch).map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO review_batches(batch_id, batch_json) VALUES ($1,$2)
                     ON CONFLICT (batch_id) DO UPDATE SET batch_json=EXCLUDED.batch_json",
                    &[&batch_id, &json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
    }

    fn persist_item(&self, item_id: &str) -> StoreResult<()> {
        let item = self.memory.get_review_item(item_id)?;
        let json = serde_json::to_string(&item).map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO review_items(item_id, batch_id, item_json) VALUES ($1,$2,$3)
                     ON CONFLICT (item_id) DO UPDATE SET item_json=EXCLUDED.item_json, batch_id=EXCLUDED.batch_id",
                    &[&item_id, &item.batch_id, &json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
    }

    fn persist_annotation(&self, annotation_id: &str) -> StoreResult<()> {
        let ann = self.memory.get_annotation(annotation_id)?;
        let json = serde_json::to_string(&ann).map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO annotations(annotation_id, item_id, annotation_json) VALUES ($1,$2,$3)
                     ON CONFLICT (annotation_id) DO UPDATE SET annotation_json=EXCLUDED.annotation_json",
                    &[&annotation_id, &ann.item_id, &json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
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
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO extract_results(result_id, result_json) VALUES ($1,$2)
                     ON CONFLICT (result_id) DO UPDATE SET result_json=EXCLUDED.result_json",
                    &[&result_id, &json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
    }
}

fn migrate(client: &mut Client) -> StoreResult<()> {
    client
        .batch_execute(
            r#"
            CREATE TABLE IF NOT EXISTS jobs (
              job_id TEXT PRIMARY KEY,
              status_json TEXT NOT NULL,
              request_json TEXT NOT NULL,
              attempt INT NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS results (
              job_id TEXT PRIMARY KEY,
              result_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
              id BIGSERIAL PRIMARY KEY,
              job_id TEXT NOT NULL,
              ts BIGINT NOT NULL,
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
              id BIGSERIAL PRIMARY KEY,
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
              id BIGSERIAL PRIMARY KEY,
              ts BIGINT NOT NULL,
              workspace_id TEXT,
              actor TEXT,
              action TEXT NOT NULL,
              detail TEXT
            );
            "#,
        )
        .map_err(|e| StoreError::Internal(e.to_string()))?;
    Ok(())
}

impl JobStore for PostgresJobStore {
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
        let id = status.job_id.clone();
        self.memory.update_job(status)?;
        self.persist_job(&id)
    }

    fn set_result(&self, result: RemoteJobResult) -> StoreResult<()> {
        let id = result.job_id.clone();
        self.memory.set_result(result)?;
        self.persist_result(&id)
    }

    fn get_result(&self, job_id: &str) -> StoreResult<RemoteJobResult> {
        self.memory.get_result(job_id)
    }

    fn append_event(&self, event: RemoteJobEvent) -> StoreResult<()> {
        let json =
            serde_json::to_string(&event).map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO events(job_id, ts, event_json) VALUES ($1,$2,$3)",
                    &[&event.job_id, &(event.ts as i64), &json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })?;
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
        self.memory.init_upload(request, public_base)
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
        self.memory.mark_upload_bytes(upload_id, size, checksum)
    }

    fn complete_upload(
        &self,
        request: RemoteUploadCompleteRequest,
        public_base: &str,
    ) -> StoreResult<RemoteUploadCompleteResponse> {
        let resp = self.memory.complete_upload(request, public_base)?;
        self.persist_asset(&resp.asset.id)?;
        Ok(resp)
    }

    fn abort_upload(&self, upload_id: &str) -> StoreResult<()> {
        self.memory.abort_upload(upload_id)
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
        let json =
            serde_json::to_string(&record).map_err(|e| StoreError::Internal(e.to_string()))?;
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO dead_letters(record_json) VALUES ($1)",
                    &[&json],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })?;
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
        let ann = self.memory.upsert_annotation(annotation)?;
        self.persist_annotation(&ann.annotation_id)?;
        Ok(ann)
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
        let id = summary.result_id.clone();
        let out = self.memory.upsert_extract_result(summary)?;
        self.persist_extract(&id)?;
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
        self.with_client(|client| {
            client
                .execute(
                    "INSERT INTO audit_log(ts, workspace_id, actor, action, detail)
                     VALUES ($1,$2,$3,$4,$5)",
                    &[&ts, &workspace_id, &actor, &action, &detail],
                )
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            Ok(())
        })
    }
}
