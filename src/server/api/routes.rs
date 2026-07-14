//! `/v1/*` 路由实现。

use std::convert::Infallible;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;

use crate::remote::catalog::{RemotePage, RemotePageQuery, RemoteReviewBatchSummary};
use crate::remote::contract::{RemoteApiErrorCode, RemoteEnvelope};
use crate::remote::models::{
    CreateRemoteBatchRequest, RemoteAnnotation, RemoteBatch, RemoteReviewItem,
    UpdateRemoteReviewItemRequest,
};
use crate::remote::types::{
    now_unix, RemoteHealth, RemoteJobPhase, RemoteJobRequest, RemoteJobResult, RemoteJobSource,
    RemoteJobStatus, REMOTE_SCHEMA_VERSION,
};
use crate::remote::upload::{
    RemoteUploadAbortRequest, RemoteUploadCompleteRequest, RemoteUploadInitRequest,
};
use crate::remote::worker_policy::WorkerHeartbeatRequest;
use crate::server::api::{request_id_from_headers, ApiError, ApiResult};
use crate::server::auth::{authorize, enforce_workspace, AuthContext};
use crate::server::object_store::sha256_hex;
use crate::server::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LimitQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    pub after: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct PageQueryParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub workspace_id: Option<String>,
    pub cursor: Option<String>,
}

impl From<PageQueryParams> for RemotePageQuery {
    fn from(value: PageQueryParams) -> Self {
        Self {
            limit: value.limit,
            offset: value.offset,
            workspace_id: value.workspace_id,
            cursor: value.cursor,
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/jobs", post(submit_job).get(list_jobs))
        .route("/v1/jobs/{id}", get(get_job))
        .route("/v1/jobs/{id}/result", get(get_result))
        .route("/v1/jobs/{id}/cancel", post(cancel_job))
        .route("/v1/jobs/{id}/events", get(job_events_sse))
        .route("/v1/jobs/{id}/events/poll", get(job_events_poll))
        .route("/v1/uploads:init", post(uploads_init))
        .route("/v1/uploads:complete", post(uploads_complete))
        .route("/v1/uploads:abort", post(uploads_abort))
        .route("/v1/uploads/{id}/bytes", put(uploads_put_bytes))
        .route("/v1/artifacts/{id}/download", get(artifact_download))
        .route("/v1/artifacts/{id}/content", get(artifact_content))
        .route("/v1/assets", get(list_assets))
        .route(
            "/v1/review/batches",
            post(create_batch).get(list_review_batches),
        )
        .route("/v1/review/batches/{id}", get(get_batch))
        .route("/v1/review/batches/{id}/items", get(list_items))
        .route("/v1/review/items/{id}", put(update_item))
        .route(
            "/v1/review/items/{id}/annotations",
            put(upsert_annotation).get(list_annotations),
        )
        .route("/v1/video/batches", get(list_video_batches))
        .route("/v1/extract/results", get(list_extract_results))
        .route("/v1/worker/heartbeat", post(worker_heartbeat))
        .with_state(state)
}

fn check_access(state: &AppState, headers: &HeaderMap, request_id: &str) -> ApiResult<AuthContext> {
    if !state.rate_limiter.check(headers) {
        return Err(ApiError::rate_limited(request_id));
    }
    authorize(&state.config, headers).map_err(|mut err| {
        err.body.request_id = Some(request_id.to_string());
        err
    })
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let pending = state.queue.pending_len().unwrap_or(0);
    Json(RemoteHealth {
        ok: true,
        message: format!("imgforge-server ok; pending={pending}"),
        server_version: Some(env!("CARGO_PKG_VERSION").into()),
        schema_version: Some(REMOTE_SCHEMA_VERSION),
    })
}

async fn submit_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut request): Json<RemoteJobRequest>,
) -> ApiResult<Json<RemoteJobStatus>> {
    let request_id = request_id_from_headers(&headers);
    let auth = check_access(&state, &headers, &request_id)?;
    if request.schema_version > REMOTE_SCHEMA_VERSION {
        return Err(ApiError::other(
            StatusCode::BAD_REQUEST,
            RemoteApiErrorCode::UnsupportedSchema,
            format!("unsupported schema {}", request.schema_version),
            &request_id,
        ));
    }
    if let Some(workspace_id) = request.workspace_id.as_deref() {
        enforce_workspace(&auth.workspace_id, Some(workspace_id), request_id.clone())?;
    } else {
        request.workspace_id = Some(auth.workspace_id.clone());
    }
    let workspace_id = request.workspace_id.clone();
    let status = state
        .store
        .create_job(request)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    let _ = state.store.append_audit(
        workspace_id.as_deref(),
        auth.actor.as_deref(),
        "job.submit",
        Some(&format!(
            "job_id={}; token_present={}; rate_limit=enforced",
            status.job_id, auth.token_present
        )),
    );
    state
        .queue
        .enqueue(&status.job_id, 0)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(status))
}

async fn list_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let jobs = state
        .store
        .list_jobs(q.limit)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(serde_json::json!({ "jobs": jobs })))
}

async fn get_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<RemoteJobStatus>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let status = state
        .store
        .get_job(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(status))
}

async fn get_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<RemoteJobResult>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let result = state
        .store
        .get_result(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(result))
}

async fn cancel_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<RemoteJobStatus>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let mut status = state
        .store
        .get_job(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    if !status.phase.is_terminal() {
        status.phase = RemoteJobPhase::Cancelled;
        status.updated_at = now_unix();
        state
            .store
            .update_job(status.clone())
            .map_err(|e| ApiError::from_store(e, &request_id))?;
    }
    Ok(Json(status))
}

async fn job_events_poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    // 确保任务存在
    let _ = state
        .store
        .get_job(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    let events = state
        .store
        .list_events(&id, q.after)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(serde_json::json!({ "events": events })))
}

async fn job_events_sse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let _ = state
        .store
        .get_job(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);
    let store = state.store.clone();
    let job_id = id.clone();
    let mut after = q.after.unwrap_or(0);

    tokio::spawn(async move {
        // 初始快照
        if let Ok(events) = store.list_events(&job_id, Some(after)) {
            for ev in events {
                after = after.max(ev.ts);
                if let Ok(data) = serde_json::to_string(&ev) {
                    let _ = tx.send(Ok(Event::default().event("job").data(data))).await;
                }
            }
        }
        // 简易轮询推送（生产可换广播 channel）
        for _ in 0..120 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Ok(events) = store.list_events(&job_id, Some(after)) {
                for ev in events {
                    after = after.max(ev.ts);
                    if let Ok(data) = serde_json::to_string(&ev) {
                        if tx
                            .send(Ok(Event::default().event("job").data(data)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            if let Ok(status) = store.get_job(&job_id) {
                if status.phase.is_terminal() {
                    break;
                }
            }
        }
    });

    let stream = ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn uploads_init(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut request): Json<RemoteUploadInitRequest>,
) -> ApiResult<Json<crate::remote::upload::RemoteUploadSession>> {
    let request_id = request_id_from_headers(&headers);
    let auth = check_access(&state, &headers, &request_id)?;
    if let Some(workspace_id) = request.workspace_id.as_deref() {
        enforce_workspace(&auth.workspace_id, Some(workspace_id), request_id.clone())?;
    } else {
        request.workspace_id = Some(auth.workspace_id.clone());
    }
    let session = state
        .store
        .init_upload(request, &state.config.public_base)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    let _ = state.store.append_audit(
        Some(&auth.workspace_id),
        auth.actor.as_deref(),
        "upload.init",
        Some(&format!(
            "upload_id={}; token_present={}",
            session.upload_id, auth.token_present
        )),
    );
    Ok(Json(session))
}

async fn uploads_put_bytes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let request_id = request_id_from_headers(&headers);
    let auth = check_access(&state, &headers, &request_id)?;
    let upload = state
        .store
        .get_upload(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    enforce_workspace(
        &auth.workspace_id,
        upload.init.workspace_id.as_deref(),
        request_id.clone(),
    )?;
    if body.len() as u64 > state.config.max_upload_bytes {
        return Err(ApiError::other(
            StatusCode::PAYLOAD_TOO_LARGE,
            RemoteApiErrorCode::RateLimited,
            format!(
                "upload exceeds max_upload_bytes ({})",
                state.config.max_upload_bytes
            ),
            request_id,
        ));
    }
    let checksum = sha256_hex(&body);
    let size = body.len() as u64;
    state
        .objects
        .put_bytes(&upload.object_key, body.to_vec())
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    state
        .store
        .mark_upload_bytes(&id, size, Some(checksum.clone()))
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "upload_id": id,
        "size": size,
        "checksum": checksum,
    })))
}

async fn uploads_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RemoteUploadCompleteRequest>,
) -> ApiResult<Json<crate::remote::upload::RemoteUploadCompleteResponse>> {
    let request_id = request_id_from_headers(&headers);
    let auth = check_access(&state, &headers, &request_id)?;
    let upload = state
        .store
        .get_upload(&request.upload_id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    enforce_workspace(
        &auth.workspace_id,
        upload.init.workspace_id.as_deref(),
        request_id.clone(),
    )?;
    let resp = state
        .store
        .complete_upload(request.clone(), &state.config.public_base)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    // 将临时 uploads/{id} 搬到 assets/{asset_id}
    let asset_key = format!("assets/{}", resp.asset.id);
    if let Ok(bytes) = state.objects.get_bytes(&upload.object_key) {
        let _ = state.objects.put_bytes(&asset_key, bytes);
        let _ = state.objects.delete(&upload.object_key);
    }
    Ok(Json(resp))
}

async fn uploads_abort(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RemoteUploadAbortRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let request_id = request_id_from_headers(&headers);
    let auth = check_access(&state, &headers, &request_id)?;
    if let Ok(upload) = state.store.get_upload(&request.upload_id) {
        enforce_workspace(
            &auth.workspace_id,
            upload.init.workspace_id.as_deref(),
            request_id.clone(),
        )?;
        let _ = state.objects.delete(&upload.object_key);
    }
    state
        .store
        .abort_upload(&request.upload_id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "upload_id": request.upload_id,
    })))
}

async fn artifact_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::remote::upload::RemoteDownloadCredential>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let cred = state
        .store
        .download_credential(&id, &state.config.public_base)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(cred))
}

async fn artifact_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let asset = state
        .store
        .get_asset(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    let key = format!("assets/{id}");
    let bytes = state
        .objects
        .get_bytes(&key)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    let mut resp = Response::new(bytes.into());
    *resp.status_mut() = StatusCode::OK;
    let headers_mut = resp.headers_mut();
    if let Some(mime) = &asset.mime {
        if let Ok(v) = mime.parse() {
            headers_mut.insert(header::CONTENT_TYPE, v);
        }
    } else {
        headers_mut.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/octet-stream"),
        );
    }
    headers_mut.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", asset.name))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );
    Ok(resp)
}

async fn list_assets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQueryParams>,
) -> ApiResult<Json<crate::remote::catalog::RemotePage<crate::remote::catalog::RemoteAssetListItem>>>
{
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let page = state
        .store
        .list_assets(q.into())
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(page))
}

async fn list_review_batches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQueryParams>,
) -> ApiResult<
    Json<crate::remote::catalog::RemotePage<crate::remote::catalog::RemoteReviewBatchSummary>>,
> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let page = state
        .store
        .list_review_batches(q.into())
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(page))
}

async fn create_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut request): Json<CreateRemoteBatchRequest>,
) -> ApiResult<Json<RemoteBatch>> {
    let request_id = request_id_from_headers(&headers);
    let auth = check_access(&state, &headers, &request_id)?;
    if let Some(workspace_id) = request.workspace_id.as_deref() {
        enforce_workspace(&auth.workspace_id, Some(workspace_id), request_id.clone())?;
    } else {
        request.workspace_id = Some(auth.workspace_id.clone());
    }
    let workspace_id = request.workspace_id.clone();
    let batch = state
        .store
        .create_batch(request)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    let _ = state.store.append_audit(
        workspace_id.as_deref(),
        auth.actor.as_deref(),
        "batch.create",
        Some(&format!(
            "batch_id={}; token_present={}",
            batch.batch_id, auth.token_present
        )),
    );
    Ok(Json(batch))
}

async fn get_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<RemoteBatch>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let batch = state
        .store
        .get_batch(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(batch))
}

async fn list_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let items = state
        .store
        .list_review_items(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn update_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateRemoteReviewItemRequest>,
) -> ApiResult<Json<RemoteReviewItem>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let item = state
        .store
        .update_review_item(&id, request)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(item))
}

async fn upsert_annotation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut annotation): Json<RemoteAnnotation>,
) -> ApiResult<Json<RemoteAnnotation>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    if annotation.item_id.is_empty() {
        annotation.item_id = id;
    } else if annotation.item_id != id {
        return Err(ApiError::other(
            StatusCode::BAD_REQUEST,
            RemoteApiErrorCode::Validation,
            "annotation item_id does not match path",
            request_id,
        ));
    }
    let annotation = state
        .store
        .upsert_annotation(annotation)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(annotation))
}

async fn list_annotations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let annotations = state
        .store
        .list_annotations(&id)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(serde_json::json!({ "annotations": annotations })))
}

async fn list_video_batches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQueryParams>,
) -> ApiResult<Json<RemotePage<RemoteReviewBatchSummary>>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let all_query = RemotePageQuery {
        limit: 10_000,
        offset: 0,
        workspace_id: q.workspace_id,
        cursor: q.cursor,
    };
    let page = state
        .store
        .list_review_batches(all_query)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    let mut items: Vec<_> = page
        .items
        .into_iter()
        .filter(|b| b.source == RemoteJobSource::VideoReview)
        .collect();
    let total = items.len();
    items = items
        .into_iter()
        .skip(q.offset)
        .take(q.limit.max(1))
        .collect();
    Ok(Json(RemotePage::new(items, total)))
}

async fn list_extract_results(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQueryParams>,
) -> ApiResult<
    Json<crate::remote::catalog::RemotePage<crate::remote::catalog::RemoteExtractResultSummary>>,
> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    let page = state
        .store
        .list_extract_results(q.into())
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    Ok(Json(page))
}

async fn worker_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<WorkerHeartbeatRequest>,
) -> ApiResult<Json<RemoteEnvelope<serde_json::Value>>> {
    let request_id = request_id_from_headers(&headers);
    let _auth = check_access(&state, &headers, &request_id)?;
    state
        .queue
        .heartbeat(&req.claim_token, state.policy.lease_secs)
        .map_err(|e| ApiError::from_store(e, &request_id))?;
    if let (Some(progress), Ok(mut status)) = (req.progress, state.store.get_job(&req.job_id)) {
        status.progress = Some(progress);
        if let Some(processed) = req.processed {
            status.processed = processed;
        }
        status.updated_at = now_unix();
        let _ = state.store.update_job(status);
    }
    Ok(Json(RemoteEnvelope::new(
        serde_json::json!({ "ok": true }),
        Some(request_id),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::config::ServerConfig;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as HttpStatus};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_ok() {
        let mut cfg = ServerConfig::default();
        cfg.inline_worker = false;
        let app = crate::server::api::app(AppState::in_memory(cfg));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), HttpStatus::OK);
    }
}
