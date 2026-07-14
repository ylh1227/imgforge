//! 远端服务器接入：控制面契约、数据面上传、任务同步与离线缓存。
//!
//! 本地能力不依赖服务器；远端为可选 backend。
//!
//! REST 控制面（schema v1）：
//! - `GET  /v1/health`
//! - `POST /v1/jobs` / `GET /v1/jobs` / `GET /v1/jobs/{id}`
//! - `GET  /v1/jobs/{id}/result` / `POST /v1/jobs/{id}/cancel`
//! - `GET  /v1/jobs/{id}/events`（SSE，可选）
//!
//! 数据面：
//! - `POST /v1/uploads:init` / `:complete` / `:abort`
//! - `GET  /v1/artifacts/{id}/download`
//!
//! 数据加载：
//! - `GET /v1/assets` / `/v1/review/batches` / `/v1/extract/results`

pub mod asset_cache;
pub mod cache;
pub mod catalog;
pub mod client;
pub mod config;
pub mod contract;
pub mod error;
pub mod events;
pub mod fetch;
pub mod http;
pub mod models;
pub mod module_bridge;
pub mod services;
pub mod task_sync;
pub mod types;
#[cfg(feature = "review")]
pub mod ui_adapter;
pub mod upload;
pub mod worker_policy;

pub use asset_cache::RemoteAssetCache;
pub use cache::{CachedArtifact, RemoteCacheStore, RemoteJobCache};
pub use catalog::{
    RemoteAssetListItem, RemoteExtractResultSummary, RemotePage, RemotePageQuery,
    RemoteReviewBatchSummary, ASSETS_PATH, EXTRACT_RESULTS_PATH, REVIEW_BATCHES_PATH,
};
pub use client::{
    build_client, try_build_http_client, DisabledRemoteClient, MockRemoteClient, RemoteClient,
};
pub use config::{RemoteAuthMode, RemoteConfig};
pub use contract::{RemoteApiErrorBody, RemoteApiErrorCode, RemoteEnvelope, RemoteResponseMeta};
pub use error::{RemoteError, RemoteResult};
pub use events::{
    RemoteJobEvent, RemoteJobEventKind, RemoteStatusTransport, JOB_EVENTS_PATH_TEMPLATE,
};
pub use fetch::{DataSource, RemoteFetch};
pub use http::HttpRemoteClient;
pub use models::{
    ConvertJobSpec, CreateRemoteBatchRequest, DataExtractJobSpec, RemoteAnnotation,
    RemoteAnnotationKind, RemoteArtifact, RemoteAsset, RemoteBatch, RemoteBatchKind,
    RemoteExtractDataset, RemoteJobSpec, RemoteReport, RemoteReviewItem, RemoteReviewItemStatus,
    ReviewJobSpec, UpdateRemoteReviewItemRequest, VideoReviewJobSpec,
    REVIEW_ANNOTATIONS_PATH_TEMPLATE, REVIEW_ITEMS_PATH_TEMPLATE, VIDEO_BATCHES_PATH,
};
pub use module_bridge::{
    create_remote_batch_from_paths, ensure_remote_asset_local, fetch_batch_items_with_thumbs,
    list_remote_annotations, list_remote_extract_results, list_remote_review_batches,
    list_remote_review_items, probe_remote_health, remote_enabled, save_annotation,
    submit_module_job, sync_review_item, upload_paths_as_assets,
};
pub use services::{RemoteAssetService, RemoteCatalogService, RemoteJobService};
pub use task_sync::{RemoteConvertOutcome, SyncSnapshot, TaskSyncService};
pub use types::{
    RemoteAssetRef, RemoteHealth, RemoteJobParams, RemoteJobPhase, RemoteJobRequest,
    RemoteJobResult, RemoteJobSource, RemoteJobStatus, RemoteJobSummary, REMOTE_SCHEMA_VERSION,
};
#[cfg(feature = "review")]
pub use ui_adapter::{
    batch_from_summary, image_from_remote_item, local_status_to_remote, placeholder_path_for_asset,
    remote_status_to_local, stats_from_items, RemoteIdMap,
};
pub use upload::{
    RemoteDownloadCredential, RemotePresignedPart, RemoteUploadAbortRequest,
    RemoteUploadCompleteRequest, RemoteUploadCompleteResponse, RemoteUploadInitRequest,
    RemoteUploadProtocol, RemoteUploadSession,
};
pub use worker_policy::{
    verify_checksum, ArtifactChecksumStatus, DeadLetterRecord, IdempotencyKey,
    WorkerHeartbeatRequest, WorkerLease, WorkerReliabilityPolicy,
};
