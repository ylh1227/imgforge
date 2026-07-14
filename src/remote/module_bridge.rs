//! Pragmatic bridge helpers for non-convert modules using the remote SDK.

use std::path::PathBuf;

use crate::remote::asset_cache::RemoteAssetCache;
use crate::remote::catalog::{
    RemoteExtractResultSummary, RemotePageQuery, RemoteReviewBatchSummary,
};
use crate::remote::client::try_build_http_client;
use crate::remote::config::RemoteConfig;
use crate::remote::error::{RemoteError, RemoteResult};
use crate::remote::models::{
    CreateRemoteBatchRequest, RemoteAnnotation, RemoteBatch, RemoteBatchKind, RemoteReviewItem,
    RemoteReviewItemStatus, UpdateRemoteReviewItemRequest,
};
use crate::remote::services::{
    job_request, RemoteAssetService, RemoteCatalogService, RemoteJobService,
};
use crate::remote::types::{
    now_unix, RemoteAssetRef, RemoteJobPhase, RemoteJobResult, RemoteJobSource, RemoteJobStatus,
    REMOTE_SCHEMA_VERSION,
};

pub fn remote_enabled(cfg: &RemoteConfig) -> bool {
    cfg.enabled && cfg.is_configured()
}

pub fn upload_paths_as_assets(
    cfg: &RemoteConfig,
    paths: &[PathBuf],
) -> RemoteResult<Vec<RemoteAssetRef>> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let assets = RemoteAssetService::new(cfg.clone(), client);
    paths
        .iter()
        .map(|path| assets.upload_file(path, cfg.workspace_id.as_deref()))
        .collect()
}

pub fn submit_module_job(
    cfg: &RemoteConfig,
    source: RemoteJobSource,
    inputs: Vec<RemoteAssetRef>,
    extras: Vec<(String, String)>,
) -> RemoteResult<(RemoteJobStatus, RemoteJobResult)> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let jobs = RemoteJobService::new(client);
    let mut request = job_request(source, inputs, cfg.workspace_id.clone());
    request.params.extras = extras;
    let status = jobs.submit(request)?;
    let result = if status.phase.is_terminal() {
        jobs.result(&status.job_id)?
    } else {
        pending_result(&status)
    };
    Ok((status, result))
}

pub fn create_remote_batch_from_paths(
    cfg: &RemoteConfig,
    name: &str,
    kind: RemoteBatchKind,
    paths: &[PathBuf],
) -> RemoteResult<RemoteBatch> {
    ensure_enabled(cfg)?;
    let assets = upload_paths_as_assets(cfg, paths)?;
    let client = try_build_http_client(cfg)?;
    let catalog = RemoteCatalogService::new(client);
    catalog.create_batch(CreateRemoteBatchRequest {
        schema_version: REMOTE_SCHEMA_VERSION,
        name: name.to_string(),
        kind,
        workspace_id: cfg.workspace_id.clone(),
        asset_ids: assets.into_iter().map(|asset| asset.id).collect(),
    })
}

pub fn sync_review_item(
    cfg: &RemoteConfig,
    item_id: &str,
    status: Option<RemoteReviewItemStatus>,
    remark: Option<String>,
    tags: Option<Vec<String>>,
) -> RemoteResult<RemoteReviewItem> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let catalog = RemoteCatalogService::new(client);
    catalog.update_review_item(
        item_id,
        UpdateRemoteReviewItemRequest {
            status,
            remark,
            tags,
        },
    )
}

pub fn list_remote_review_items(
    cfg: &RemoteConfig,
    batch_id: &str,
) -> RemoteResult<Vec<RemoteReviewItem>> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let catalog = RemoteCatalogService::new(client);
    catalog.list_review_items(batch_id)
}

pub fn list_remote_review_batches(
    cfg: &RemoteConfig,
    kind: RemoteBatchKind,
) -> RemoteResult<Vec<RemoteReviewBatchSummary>> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let catalog = RemoteCatalogService::new(client);
    let query = RemotePageQuery {
        limit: 200,
        workspace_id: cfg.workspace_id.clone(),
        ..Default::default()
    };
    let page = match kind {
        RemoteBatchKind::Image => catalog.list_review_batches(query)?,
        RemoteBatchKind::Video => catalog.list_video_batches(query)?,
    };
    Ok(page.items)
}

pub fn list_remote_extract_results(
    cfg: &RemoteConfig,
) -> RemoteResult<Vec<RemoteExtractResultSummary>> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let catalog = RemoteCatalogService::new(client);
    let page = catalog.list_extract_results(RemotePageQuery {
        limit: 200,
        workspace_id: cfg.workspace_id.clone(),
        ..Default::default()
    })?;
    Ok(page.items)
}

pub fn list_remote_annotations(
    cfg: &RemoteConfig,
    item_id: &str,
) -> RemoteResult<Vec<RemoteAnnotation>> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let catalog = RemoteCatalogService::new(client);
    catalog.list_annotations(item_id)
}

/// 拉取批次条目，并把 thumb_asset 缓存到本地（若有）。
pub fn fetch_batch_items_with_thumbs(
    cfg: &RemoteConfig,
    batch_id: &str,
) -> RemoteResult<Vec<(RemoteReviewItem, Option<PathBuf>)>> {
    ensure_enabled(cfg)?;
    let items = list_remote_review_items(cfg, batch_id)?;
    let cache = RemoteAssetCache::from_config(cfg);
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let thumb_path = item
            .thumb_asset
            .as_ref()
            .and_then(|a| cache.ensure_local(cfg, a).ok());
        out.push((item, thumb_path));
    }
    Ok(out)
}

pub fn ensure_remote_asset_local(
    cfg: &RemoteConfig,
    asset: &RemoteAssetRef,
) -> RemoteResult<PathBuf> {
    ensure_enabled(cfg)?;
    RemoteAssetCache::from_config(cfg).ensure_local(cfg, asset)
}

pub fn save_annotation(
    cfg: &RemoteConfig,
    annotation: RemoteAnnotation,
) -> RemoteResult<RemoteAnnotation> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let catalog = RemoteCatalogService::new(client);
    catalog.upsert_annotation(annotation)
}

pub fn probe_remote_health(cfg: &RemoteConfig) -> RemoteResult<String> {
    ensure_enabled(cfg)?;
    let client = try_build_http_client(cfg)?;
    let health = client.health()?;
    if health.ok {
        Ok(health.message)
    } else {
        Err(RemoteError::Other(format!(
            "health not ok: {}",
            health.message
        )))
    }
}

fn ensure_enabled(cfg: &RemoteConfig) -> RemoteResult<()> {
    if remote_enabled(cfg) {
        Ok(())
    } else if cfg.enabled {
        Err(RemoteError::NotConfigured("缺少 remote.base_url".into()))
    } else {
        Err(RemoteError::Disabled)
    }
}

fn pending_result(status: &RemoteJobStatus) -> RemoteJobResult {
    RemoteJobResult {
        schema_version: REMOTE_SCHEMA_VERSION,
        job_id: status.job_id.clone(),
        phase: match status.phase {
            RemoteJobPhase::Queued
            | RemoteJobPhase::Running
            | RemoteJobPhase::Succeeded
            | RemoteJobPhase::Failed
            | RemoteJobPhase::Cancelled
            | RemoteJobPhase::Unknown => status.phase,
        },
        successes: 0,
        failures: 0,
        artifacts: Vec::new(),
        error_summary: status.error_summary.clone(),
        updated_at: now_unix(),
    }
}
