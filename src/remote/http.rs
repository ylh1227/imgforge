//! 基于 reqwest blocking 的真实 HTTP RemoteClient。
//!
//! 约定 REST 路径（schema v1）：
//! - `GET  /v1/health`
//! - `POST /v1/jobs`
//! - `GET  /v1/jobs?limit=`
//! - `GET  /v1/jobs/{id}`
//! - `GET  /v1/jobs/{id}/result`
//! - `POST /v1/jobs/{id}/cancel`
//! - `GET  /v1/jobs/{id}/events`（SSE；客户端可轮询兼容）
//! - `POST /v1/uploads:init` / `:complete` / `:abort`
//! - `GET  /v1/artifacts/{id}/download`
//! - `GET  /v1/assets` / `/v1/review/batches` / `/v1/extract/results`
//! - `GET  /v1/video/batches`
//! - `POST /v1/review/batches` / `GET /v1/review/batches/{id}`
//! - `GET  /v1/review/batches/{id}/items`
//! - `PUT /v1/review/items/{id}` / annotations under `/v1/review/items/{id}/annotations`
//!
//! 错误体：`{ code, message, retryable, details?, request_id? }`
//! 对 429 / 5xx / retryable API 错误做有限次指数退避重试。

use std::thread;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde::Deserialize;

use crate::remote::catalog::{
    RemoteAssetListItem, RemoteExtractResultSummary, RemotePage, RemotePageQuery,
    RemoteReviewBatchSummary, ASSETS_PATH, EXTRACT_RESULTS_PATH, REVIEW_BATCHES_PATH,
};
use crate::remote::config::RemoteConfig;
use crate::remote::contract::RemoteApiErrorBody;
use crate::remote::error::{RemoteError, RemoteResult};
use crate::remote::events::RemoteJobEvent;
use crate::remote::models::{
    CreateRemoteBatchRequest, RemoteAnnotation, RemoteBatch, RemoteReviewItem,
    UpdateRemoteReviewItemRequest, REVIEW_ANNOTATIONS_PATH_TEMPLATE, REVIEW_ITEMS_PATH_TEMPLATE,
    VIDEO_BATCHES_PATH,
};
use crate::remote::types::{
    RemoteHealth, RemoteJobRequest, RemoteJobResult, RemoteJobStatus, RemoteJobSummary,
    REMOTE_SCHEMA_VERSION,
};
use crate::remote::upload::{
    RemoteDownloadCredential, RemoteUploadAbortRequest, RemoteUploadCompleteRequest,
    RemoteUploadCompleteResponse, RemoteUploadInitRequest, RemoteUploadSession,
};
use crate::remote::RemoteClient;

const MAX_RETRIES: u32 = 3;
const RETRY_BASE_MS: u64 = 200;

#[derive(Debug, Clone)]
pub struct HttpRemoteClient {
    http: Client,
    base_url: String,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JobListResponse {
    #[serde(default)]
    jobs: Vec<RemoteJobSummary>,
}

#[derive(Debug, Deserialize)]
struct EventsResponse {
    #[serde(default)]
    events: Vec<RemoteJobEvent>,
}

#[derive(Debug, Deserialize)]
struct ReviewItemsResponse {
    #[serde(default)]
    items: Vec<RemoteReviewItem>,
}

#[derive(Debug, Deserialize)]
struct AnnotationsResponse {
    #[serde(default, alias = "items")]
    annotations: Vec<RemoteAnnotation>,
}

impl HttpRemoteClient {
    pub fn try_new(config: &RemoteConfig) -> RemoteResult<Self> {
        if !config.enabled {
            return Err(RemoteError::Disabled);
        }
        let Some(base) = config
            .base_url
            .as_ref()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
        else {
            return Err(RemoteError::NotConfigured("缺少 remote.base_url".into()));
        };

        let http = Client::builder()
            .timeout(config.timeout())
            .user_agent(format!("imgforge/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| RemoteError::Request(e.to_string()))?;

        let token = config.resolve_token();
        if matches!(
            config.auth_mode,
            crate::remote::config::RemoteAuthMode::EnvBearer
                | crate::remote::config::RemoteAuthMode::Keychain
        ) && token.is_none()
        {
            return Err(RemoteError::AuthRequired);
        }

        Ok(Self {
            http,
            base_url: base,
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn apply_auth(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        if let Some(token) = &self.token {
            req.header(AUTHORIZATION, format!("Bearer {token}"))
        } else {
            req
        }
    }

    fn map_error_body(status: StatusCode, body: &str) -> RemoteError {
        if let Ok(api) = serde_json::from_str::<RemoteApiErrorBody>(body) {
            return RemoteError::from_api_body(api);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return RemoteError::AuthRequired;
        }
        if status == StatusCode::NOT_FOUND {
            let id = body.trim();
            return RemoteError::JobNotFound(if id.is_empty() {
                "unknown".into()
            } else {
                id.chars().take(120).collect()
            });
        }
        let retryable = status.as_u16() == 429 || status.is_server_error();
        if retryable {
            return RemoteError::Api {
                code: if status.as_u16() == 429 {
                    crate::remote::contract::RemoteApiErrorCode::RateLimited
                } else {
                    crate::remote::contract::RemoteApiErrorCode::Unavailable
                },
                message: format!("HTTP {status}: {}", truncate(body, 240)),
                retryable: true,
                details: None,
                request_id: None,
            };
        }
        RemoteError::Request(format!("HTTP {status}: {}", truncate(body, 240)))
    }

    fn with_retries<T, F>(&self, mut op: F) -> RemoteResult<T>
    where
        F: FnMut() -> RemoteResult<T>,
    {
        let mut attempt = 0u32;
        loop {
            match op() {
                Ok(v) => return Ok(v),
                Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                    let sleep_ms = RETRY_BASE_MS.saturating_mul(1u64 << attempt.min(8));
                    tracing::debug!(
                        attempt,
                        sleep_ms,
                        error = %e,
                        "retrying remote request"
                    );
                    thread::sleep(Duration::from_millis(sleep_ms));
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn get_json_once<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RemoteResult<T> {
        let req = self.apply_auth(self.http.get(self.url(path)));
        let resp = req
            .send()
            .map_err(|e| RemoteError::Request(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| RemoteError::Request(e.to_string()))?;
        if !status.is_success() {
            return Err(Self::map_error_body(status, &text));
        }
        serde_json::from_str(&text).map_err(|e| {
            RemoteError::Request(format!(
                "invalid JSON from {path}: {e}; body={}",
                truncate(&text, 160)
            ))
        })
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> RemoteResult<T> {
        self.with_retries(|| self.get_json_once(path))
    }

    fn post_json_once<T: for<'de> Deserialize<'de>, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> RemoteResult<T> {
        self.send_json_once(Method::POST, path, body)
    }

    fn send_json_once<T: for<'de> Deserialize<'de>, B: serde::Serialize>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> RemoteResult<T> {
        let req = self
            .apply_auth(self.http.request(method, self.url(path)))
            .header(CONTENT_TYPE, "application/json")
            .header(
                USER_AGENT,
                format!("imgforge/{}", env!("CARGO_PKG_VERSION")),
            )
            .json(body);
        let resp = req
            .send()
            .map_err(|e| RemoteError::Request(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| RemoteError::Request(e.to_string()))?;
        if !status.is_success() {
            return Err(Self::map_error_body(status, &text));
        }
        serde_json::from_str(&text).map_err(|e| {
            RemoteError::Request(format!(
                "invalid JSON from {path}: {e}; body={}",
                truncate(&text, 160)
            ))
        })
    }

    fn post_json<T: for<'de> Deserialize<'de>, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> RemoteResult<T> {
        self.with_retries(|| self.post_json_once(path, body))
    }

    fn put_json_once<T: for<'de> Deserialize<'de>, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> RemoteResult<T> {
        self.send_json_once(Method::PUT, path, body)
    }

    fn put_json<T: for<'de> Deserialize<'de>, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> RemoteResult<T> {
        self.with_retries(|| self.put_json_once(path, body))
    }

    fn get_text_once(&self, path: &str) -> RemoteResult<(StatusCode, String)> {
        let req = self.apply_auth(self.http.get(self.url(path)));
        let resp = req
            .send()
            .map_err(|e| RemoteError::Request(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| RemoteError::Request(e.to_string()))?;
        Ok((status, text))
    }
}

impl RemoteClient for HttpRemoteClient {
    fn health(&self) -> RemoteResult<RemoteHealth> {
        match self.get_json::<RemoteHealth>("/v1/health") {
            Ok(mut health) => {
                if health.schema_version.is_none() {
                    health.schema_version = Some(REMOTE_SCHEMA_VERSION);
                }
                if let Some(ver) = health.schema_version {
                    if ver > REMOTE_SCHEMA_VERSION {
                        return Err(RemoteError::UnsupportedSchema(ver));
                    }
                }
                Ok(health)
            }
            Err(RemoteError::JobNotFound(_)) => Ok(RemoteHealth {
                ok: false,
                message: "health endpoint not found".into(),
                server_version: None,
                schema_version: Some(REMOTE_SCHEMA_VERSION),
            }),
            Err(e) => Err(e),
        }
    }

    fn submit_job(&self, request: RemoteJobRequest) -> RemoteResult<RemoteJobStatus> {
        self.post_json("/v1/jobs", &request)
    }

    fn get_job(&self, job_id: &str) -> RemoteResult<RemoteJobStatus> {
        let path = format!("/v1/jobs/{}", urlencoding_path(job_id));
        self.get_json(&path)
    }

    fn list_jobs(&self, limit: usize) -> RemoteResult<Vec<RemoteJobSummary>> {
        let path = format!("/v1/jobs?limit={}", limit.max(1));
        self.with_retries(|| {
            let (status, text) = self.get_text_once(&path)?;
            if !status.is_success() {
                return Err(Self::map_error_body(status, &text));
            }
            if let Ok(list) = serde_json::from_str::<JobListResponse>(&text) {
                return Ok(list.jobs);
            }
            serde_json::from_str::<Vec<RemoteJobSummary>>(&text).map_err(|e| {
                RemoteError::Request(format!(
                    "invalid job list JSON: {e}; body={}",
                    truncate(&text, 160)
                ))
            })
        })
    }

    fn get_result(&self, job_id: &str) -> RemoteResult<RemoteJobResult> {
        let path = format!("/v1/jobs/{}/result", urlencoding_path(job_id));
        self.get_json(&path)
    }

    fn cancel_job(&self, job_id: &str) -> RemoteResult<RemoteJobStatus> {
        let path = format!("/v1/jobs/{}/cancel", urlencoding_path(job_id));
        self.post_json(&path, &serde_json::json!({}))
    }

    fn init_upload(&self, request: RemoteUploadInitRequest) -> RemoteResult<RemoteUploadSession> {
        self.post_json("/v1/uploads:init", &request)
    }

    fn complete_upload(
        &self,
        request: RemoteUploadCompleteRequest,
    ) -> RemoteResult<RemoteUploadCompleteResponse> {
        self.post_json("/v1/uploads:complete", &request)
    }

    fn abort_upload(&self, request: RemoteUploadAbortRequest) -> RemoteResult<()> {
        let _: serde_json::Value = self.post_json("/v1/uploads:abort", &request)?;
        Ok(())
    }

    fn artifact_download_url(&self, asset_id: &str) -> RemoteResult<RemoteDownloadCredential> {
        let path = format!("/v1/artifacts/{}/download", urlencoding_path(asset_id));
        self.get_json(&path)
    }

    fn upload_bytes(&self, upload_url: &str, bytes: &[u8]) -> RemoteResult<()> {
        self.with_retries(|| {
            let url = if upload_url.starts_with("http://") || upload_url.starts_with("https://") {
                upload_url.to_string()
            } else {
                self.url(upload_url)
            };
            let req = self
                .apply_auth(self.http.put(url))
                .header(CONTENT_TYPE, "application/octet-stream")
                .body(bytes.to_vec());
            let resp = req
                .send()
                .map_err(|e| RemoteError::Request(e.to_string()))?;
            let status = resp.status();
            let text = resp
                .text()
                .map_err(|e| RemoteError::Request(e.to_string()))?;
            if !status.is_success() {
                return Err(Self::map_error_body(status, &text));
            }
            Ok(())
        })
    }

    fn download_bytes(&self, download_url: &str) -> RemoteResult<Vec<u8>> {
        self.with_retries(|| {
            let url = if download_url.starts_with("http://") || download_url.starts_with("https://")
            {
                download_url.to_string()
            } else {
                self.url(download_url)
            };
            let req = self.apply_auth(self.http.get(url));
            let resp = req
                .send()
                .map_err(|e| RemoteError::Request(e.to_string()))?;
            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().unwrap_or_default();
                return Err(Self::map_error_body(status, &text));
            }
            resp.bytes()
                .map(|b| b.to_vec())
                .map_err(|e| RemoteError::Request(e.to_string()))
        })
    }

    fn list_job_events(
        &self,
        job_id: &str,
        after_ts: Option<u64>,
    ) -> RemoteResult<Vec<RemoteJobEvent>> {
        // JSON 轮询兼容路径；SSE 实时流为 `GET /v1/jobs/{id}/events`。
        let mut path = format!("/v1/jobs/{}/events/poll", urlencoding_path(job_id));
        if let Some(ts) = after_ts {
            path.push_str(&format!("?after={ts}"));
        }
        self.with_retries(|| {
            let (status, text) = self.get_text_once(&path)?;
            if !status.is_success() {
                return Err(Self::map_error_body(status, &text));
            }
            if let Ok(wrap) = serde_json::from_str::<EventsResponse>(&text) {
                return Ok(wrap.events);
            }
            serde_json::from_str::<Vec<RemoteJobEvent>>(&text).map_err(|e| {
                RemoteError::Request(format!(
                    "invalid events JSON: {e}; body={}",
                    truncate(&text, 160)
                ))
            })
        })
    }

    fn list_assets(&self, query: RemotePageQuery) -> RemoteResult<RemotePage<RemoteAssetListItem>> {
        let path = format!(
            "{}?limit={}&offset={}",
            ASSETS_PATH,
            query.limit.max(1),
            query.offset
        );
        self.get_json(&path)
    }

    fn list_review_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        let path = page_path(REVIEW_BATCHES_PATH, &query);
        self.get_json(&path)
    }

    fn list_video_batches(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteReviewBatchSummary>> {
        let path = page_path(VIDEO_BATCHES_PATH, &query);
        self.get_json(&path)
    }

    fn create_batch(&self, request: CreateRemoteBatchRequest) -> RemoteResult<RemoteBatch> {
        self.post_json(REVIEW_BATCHES_PATH, &request)
    }

    fn get_batch(&self, batch_id: &str) -> RemoteResult<RemoteBatch> {
        let path = format!("{}/{}", REVIEW_BATCHES_PATH, urlencoding_path(batch_id));
        self.get_json(&path)
    }

    fn list_review_items(&self, batch_id: &str) -> RemoteResult<Vec<RemoteReviewItem>> {
        let path = review_items_path(batch_id);
        self.with_retries(|| {
            let (status, text) = self.get_text_once(&path)?;
            if !status.is_success() {
                return Err(Self::map_error_body(status, &text));
            }
            if let Ok(wrap) = serde_json::from_str::<ReviewItemsResponse>(&text) {
                return Ok(wrap.items);
            }
            serde_json::from_str::<Vec<RemoteReviewItem>>(&text).map_err(|e| {
                RemoteError::Request(format!(
                    "invalid review items JSON: {e}; body={}",
                    truncate(&text, 160)
                ))
            })
        })
    }

    fn update_review_item(
        &self,
        item_id: &str,
        request: UpdateRemoteReviewItemRequest,
    ) -> RemoteResult<RemoteReviewItem> {
        let path = format!("/v1/review/items/{}", urlencoding_path(item_id));
        self.put_json(&path, &request)
    }

    fn upsert_annotation(&self, annotation: RemoteAnnotation) -> RemoteResult<RemoteAnnotation> {
        if annotation.item_id.trim().is_empty() {
            return Err(RemoteError::Other("annotation item_id is required".into()));
        }
        let path = review_annotations_path(&annotation.item_id);
        self.put_json(&path, &annotation)
    }

    fn list_annotations(&self, item_id: &str) -> RemoteResult<Vec<RemoteAnnotation>> {
        let path = review_annotations_path(item_id);
        self.with_retries(|| {
            let (status, text) = self.get_text_once(&path)?;
            if !status.is_success() {
                return Err(Self::map_error_body(status, &text));
            }
            if let Ok(wrap) = serde_json::from_str::<AnnotationsResponse>(&text) {
                return Ok(wrap.annotations);
            }
            serde_json::from_str::<Vec<RemoteAnnotation>>(&text).map_err(|e| {
                RemoteError::Request(format!(
                    "invalid annotations JSON: {e}; body={}",
                    truncate(&text, 160)
                ))
            })
        })
    }

    fn list_extract_results(
        &self,
        query: RemotePageQuery,
    ) -> RemoteResult<RemotePage<RemoteExtractResultSummary>> {
        let path = page_path(EXTRACT_RESULTS_PATH, &query);
        self.get_json(&path)
    }
}

fn page_path(base: &str, query: &RemotePageQuery) -> String {
    let mut params = vec![
        format!("limit={}", query.limit.max(1)),
        format!("offset={}", query.offset),
    ];
    if let Some(workspace_id) = &query.workspace_id {
        params.push(format!("workspace_id={}", urlencoding_path(workspace_id)));
    }
    if let Some(cursor) = &query.cursor {
        params.push(format!("cursor={}", urlencoding_path(cursor)));
    }
    format!("{base}?{}", params.join("&"))
}

fn review_items_path(batch_id: &str) -> String {
    REVIEW_ITEMS_PATH_TEMPLATE.replace("{id}", &urlencoding_path(batch_id))
}

fn review_annotations_path(item_id: &str) -> String {
    REVIEW_ANNOTATIONS_PATH_TEMPLATE.replace("{id}", &urlencoding_path(item_id))
}

fn truncate(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

fn urlencoding_path(id: &str) -> String {
    id.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::contract::{RemoteApiErrorBody, RemoteApiErrorCode};

    #[test]
    fn try_new_requires_enabled_and_url() {
        let disabled = RemoteConfig::default();
        assert!(matches!(
            HttpRemoteClient::try_new(&disabled),
            Err(RemoteError::Disabled)
        ));

        let missing_url = RemoteConfig {
            enabled: true,
            ..RemoteConfig::default()
        };
        assert!(matches!(
            HttpRemoteClient::try_new(&missing_url),
            Err(RemoteError::NotConfigured(_))
        ));

        let ok = RemoteConfig {
            enabled: true,
            base_url: Some("https://api.example.com/".into()),
            auth_mode: crate::remote::config::RemoteAuthMode::None,
            ..RemoteConfig::default()
        };
        let client = HttpRemoteClient::try_new(&ok).unwrap();
        assert_eq!(client.base_url, "https://api.example.com");
    }

    #[test]
    fn env_bearer_without_token_is_auth_required() {
        if std::env::var_os("IMGFORGE_REMOTE_TOKEN").is_some() {
            return;
        }
        let cfg = RemoteConfig {
            enabled: true,
            base_url: Some("https://api.example.com".into()),
            auth_mode: crate::remote::config::RemoteAuthMode::EnvBearer,
            ..RemoteConfig::default()
        };
        assert!(matches!(
            HttpRemoteClient::try_new(&cfg),
            Err(RemoteError::AuthRequired)
        ));
    }

    #[test]
    fn path_encoding_keeps_safe_chars() {
        assert_eq!(urlencoding_path("job-1_a.b~"), "job-1_a.b~");
        assert!(urlencoding_path("a/b").contains('%'));
    }

    #[test]
    fn map_error_body_parses_unified_format() {
        let body =
            RemoteApiErrorBody::new(RemoteApiErrorCode::RateLimited, "slow").request_id("r1");
        let json = serde_json::to_string(&body).unwrap();
        let err = HttpRemoteClient::map_error_body(StatusCode::TOO_MANY_REQUESTS, &json);
        assert!(err.is_retryable());
    }
}
