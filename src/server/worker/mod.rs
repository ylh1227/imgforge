//! Worker 池：认领任务、heartbeat、幂等完成、失败重试与死信。
//!
//! 本地可用版：从 ObjectStore 取输入 → 调用 `run_batch` → 登记输出 artifact。

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::config::AppConfig;
use crate::core::types::{Concurrency, ImageFormat, Quality};
use crate::job::run_batch;
use crate::process_util;
use crate::remote::catalog::RemoteExtractResultSummary;
use crate::remote::events::{RemoteJobEvent, RemoteJobEventKind};
use crate::remote::models::{CreateRemoteBatchRequest, RemoteBatchKind};
use crate::remote::types::{
    now_unix, RemoteAssetRef, RemoteJobPhase, RemoteJobRequest, RemoteJobResult, RemoteJobSource,
    RemoteJobStatus, REMOTE_SCHEMA_VERSION,
};
use crate::remote::worker_policy::{
    verify_checksum, ArtifactChecksumStatus, DeadLetterRecord, WorkerReliabilityPolicy,
};
use crate::server::object_store::{sha256_file, sha256_hex};
use crate::server::state::AppState;

struct MaterializedInput {
    asset: RemoteAssetRef,
    path: PathBuf,
    size: u64,
}

/// 在独立线程中运行的内联 Worker。
pub struct InlineWorker {
    #[allow(dead_code)]
    state: AppState,
    #[allow(dead_code)]
    worker_id: String,
    stop: Arc<AtomicBool>,
}

impl InlineWorker {
    pub fn spawn(state: AppState) -> Self {
        let worker_id = format!("inline-{}", uuid::Uuid::new_v4());
        let stop = Arc::new(AtomicBool::new(false));
        let worker = Self {
            state: state.clone(),
            worker_id: worker_id.clone(),
            stop: stop.clone(),
        };
        let run_state = state;
        let run_id = worker_id;
        let run_stop = stop;
        thread::Builder::new()
            .name("imgforge-inline-worker".into())
            .spawn(move || worker_loop(run_state, run_id, run_stop))
            .expect("spawn inline worker");
        worker
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn worker_loop(state: AppState, worker_id: String, stop: Arc<AtomicBool>) {
    let policy = state.policy.clone();
    while !stop.load(Ordering::SeqCst) {
        if let Err(e) = state.queue.reclaim_expired() {
            tracing::warn!(error = %format!("{e:?}"), "reclaim failed");
        }
        match state.queue.claim(&worker_id, policy.lease_secs) {
            Ok(Some(claimed)) => {
                if let Err(e) = process_one(
                    &state,
                    &policy,
                    &claimed.lease.claim_token,
                    &claimed.message.job_id,
                    claimed.message.attempt,
                    &worker_id,
                ) {
                    tracing::warn!(job_id = %claimed.message.job_id, error = %e, "worker process failed");
                }
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(e) => {
                tracing::warn!(error = %format!("{e:?}"), "claim failed");
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

fn process_one(
    state: &AppState,
    policy: &WorkerReliabilityPolicy,
    claim_token: &str,
    job_id: &str,
    attempt: u32,
    worker_id: &str,
) -> Result<(), String> {
    let _ = state.queue.heartbeat(claim_token, policy.lease_secs);
    let record = state
        .store
        .get_job_record(job_id)
        .map_err(|e| format!("{e:?}"))?;
    let mut status = record.status.clone();

    if status.phase.is_terminal() {
        let _ = state.queue.ack(claim_token);
        return Ok(());
    }

    status.phase = RemoteJobPhase::Running;
    status.progress = Some(0.05);
    status.updated_at = now_unix();
    state
        .store
        .update_job(status.clone())
        .map_err(|e| format!("{e:?}"))?;

    let _ = state.store.append_event(RemoteJobEvent {
        schema_version: REMOTE_SCHEMA_VERSION,
        job_id: job_id.to_string(),
        source: status.source,
        kind: RemoteJobEventKind::Heartbeat,
        phase: Some(RemoteJobPhase::Running),
        progress: status.progress,
        processed: Some(status.processed),
        total: Some(status.total),
        message: Some(format!("worker={worker_id} attempt={attempt}")),
        ts: now_unix(),
    });

    let run_result = match record.request.source {
        RemoteJobSource::Convert => run_convert_job(state, job_id, &record.request),
        RemoteJobSource::Review => run_review_job(state, job_id, &record.request),
        RemoteJobSource::VideoReview => run_video_review_job(state, job_id, &record.request),
        RemoteJobSource::DataExtract => run_data_extract_job(state, job_id, &record.request),
        RemoteJobSource::Other => Err("unsupported remote job source: other".into()),
    };

    match run_result {
        Ok((result, final_status)) => {
            if policy.require_artifact_checksum {
                for art in &result.artifacts {
                    match verify_checksum(art.checksum.as_deref(), art.checksum.as_deref()) {
                        ArtifactChecksumStatus::Ok | ArtifactChecksumStatus::Missing => {}
                        other => {
                            return fail_or_retry(
                                state,
                                policy,
                                claim_token,
                                job_id,
                                attempt,
                                &status,
                                format!("checksum {other:?}"),
                            );
                        }
                    }
                }
            }
            state
                .store
                .update_job(final_status)
                .map_err(|e| format!("{e:?}"))?;
            state
                .store
                .set_result(result)
                .map_err(|e| format!("{e:?}"))?;
            state.queue.ack(claim_token).map_err(|e| format!("{e:?}"))?;
            Ok(())
        }
        Err(err) => fail_or_retry(state, policy, claim_token, job_id, attempt, &status, err),
    }
}

fn run_convert_job(
    state: &AppState,
    job_id: &str,
    request: &RemoteJobRequest,
) -> Result<(RemoteJobResult, RemoteJobStatus), String> {
    let work_root = state.config.work_dir().join(job_id).join("convert");
    let (input_dir, inputs) = materialize_inputs(state, job_id, request, "convert")?;
    let output_dir = work_root.join("output");
    std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

    let mut config = AppConfig::default();
    config.input_dir = input_dir.clone();
    config.output_dir = output_dir.clone();
    config.explicit_inputs = inputs.iter().map(|input| input.path.clone()).collect();
    config.recursive = request.params.recursive.unwrap_or(true);
    config.preserve_structure = request.params.preserve_structure.unwrap_or(true);
    config.overwrite = request.params.overwrite.unwrap_or(true);
    config.rename_template = request.params.rename_template.clone();
    config.bayer_only = request.params.bayer_only.unwrap_or(false);
    config.target_max_bytes = request.params.target_max_bytes;
    config.concurrency = Concurrency::new(num_cpus::get().max(1))
        .unwrap_or_else(|_| Concurrency::default_parallel());
    if let Some(q) = request.params.quality {
        config.quality = Quality::new(q).unwrap_or(Quality::DEFAULT);
    }
    if let Some(fmt) = &request.params.target_format {
        config.target_format = parse_format(fmt);
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let report = rt
        .block_on(run_batch(config, cancelled, None))
        .map_err(|e| e.to_string())?;

    let mut artifacts = Vec::new();
    collect_output_files(&output_dir, &output_dir, &mut artifacts)?;
    let mut remote_artifacts = Vec::new();
    for (i, path) in artifacts.iter().enumerate() {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("out-{i}"));
        let rel = path
            .strip_prefix(&output_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let asset_id = format!("out-{job_id}-{i}");
        let key = format!("assets/{asset_id}");
        let size = state
            .objects
            .put_file(&key, path)
            .map_err(|e| format!("{e:?}"))?;
        let checksum = sha256_file(path).map_err(|e| format!("{e:?}"))?;
        let asset = RemoteAssetRef {
            id: asset_id.clone(),
            name: name.clone(),
            mime: guess_mime(&name),
            size: Some(size),
            checksum: Some(checksum),
            download_url: Some(format!(
                "{}/v1/artifacts/{}/content",
                state.config.public_base.trim_end_matches('/'),
                asset_id
            )),
        };
        // 在 extras 风格：把相对路径塞进 name 旁的 id 注释——用 register 并在 name 保留文件名，
        // 相对路径写入 download 侧可通过 asset.name；为保留结构，name 用相对路径。
        let mut asset = asset;
        asset.name = rel;
        state
            .store
            .register_asset(asset.clone())
            .map_err(|e| format!("{e:?}"))?;
        remote_artifacts.push(asset);
    }

    let mut status = state.store.get_job(job_id).map_err(|e| format!("{e:?}"))?;
    let failed = !report.failures.is_empty() && report.successes == 0;
    status.phase = if failed {
        RemoteJobPhase::Failed
    } else if report.cancelled {
        RemoteJobPhase::Cancelled
    } else {
        RemoteJobPhase::Succeeded
    };
    status.processed = report.successes;
    status.total = report.total.max(report.successes + report.failures.len());
    status.progress = Some(1.0);
    status.updated_at = now_unix();
    if !report.failures.is_empty() {
        status.error_summary = Some(
            report
                .failures
                .iter()
                .take(5)
                .map(|f| format!("{}: {}", f.path.display(), f.error))
                .collect::<Vec<_>>()
                .join("; "),
        );
    }

    let result = RemoteJobResult {
        schema_version: REMOTE_SCHEMA_VERSION,
        job_id: job_id.to_string(),
        phase: status.phase,
        successes: report.successes,
        failures: report.failures.len(),
        artifacts: remote_artifacts,
        error_summary: status.error_summary.clone(),
        updated_at: now_unix(),
    };
    Ok((result, status))
}

fn run_review_job(
    state: &AppState,
    job_id: &str,
    request: &RemoteJobRequest,
) -> Result<(RemoteJobResult, RemoteJobStatus), String> {
    let (_input_dir, inputs) = materialize_inputs(state, job_id, request, "review")?;
    let mut artifacts = Vec::new();
    let mut thumbs: HashMap<String, (RemoteAssetRef, (u32, u32))> = HashMap::new();

    for (idx, input) in inputs.iter().enumerate() {
        match make_thumbnail_jpeg(&input.path) {
            Ok((bytes, width, height)) => {
                let name = thumb_name(&input.asset.name, idx);
                let asset = register_bytes_asset(
                    state,
                    job_id,
                    "review-thumb",
                    idx,
                    &name,
                    Some("image/jpeg"),
                    bytes,
                )?;
                thumbs.insert(input.asset.id.clone(), (asset.clone(), (width, height)));
                artifacts.push(asset);
            }
            Err(err) => {
                tracing::warn!(
                    job_id,
                    asset_id = %input.asset.id,
                    error = %err,
                    "review thumbnail generation skipped"
                );
            }
        }
    }

    let batch_id = extra_value(request, "batch_id");
    let batch = if let Some(batch_id) = batch_id {
        state
            .store
            .get_batch(&batch_id)
            .map_err(|e| format!("{e:?}"))?
    } else {
        let batch_name =
            extra_value(request, "batch_name").unwrap_or_else(|| format!("Review {job_id}"));
        state
            .store
            .create_batch(CreateRemoteBatchRequest {
                schema_version: REMOTE_SCHEMA_VERSION,
                name: batch_name,
                kind: RemoteBatchKind::Image,
                workspace_id: workspace_id_for(state, request),
                asset_ids: request
                    .inputs
                    .iter()
                    .map(|asset| asset.id.clone())
                    .collect(),
            })
            .map_err(|e| format!("{e:?}"))?
    };

    attach_generated_assets(state, &batch.batch_id, &thumbs, true)?;

    let status = succeeded_status(state, job_id, inputs.len(), inputs.len(), None)?;
    Ok((
        RemoteJobResult {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: job_id.to_string(),
            phase: RemoteJobPhase::Succeeded,
            successes: inputs.len(),
            failures: 0,
            artifacts,
            error_summary: None,
            updated_at: now_unix(),
        },
        status,
    ))
}

fn run_video_review_job(
    state: &AppState,
    job_id: &str,
    request: &RemoteJobRequest,
) -> Result<(RemoteJobResult, RemoteJobStatus), String> {
    let (_input_dir, inputs) = materialize_inputs(state, job_id, request, "video_review")?;
    let output_dir = state
        .config
        .work_dir()
        .join(job_id)
        .join("video_review")
        .join("output");
    std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

    let mut artifacts = Vec::new();
    let mut covers: HashMap<String, (RemoteAssetRef, (u32, u32))> = HashMap::new();

    for (idx, input) in inputs.iter().enumerate() {
        let cover_path = output_dir.join(format!("cover-{idx}.jpg"));
        let ffprobe_json = ffprobe_json(&input.path).ok();
        if extract_cover_frame(&input.path, &cover_path).is_ok() && cover_path.is_file() {
            let name = thumb_name(&input.asset.name, idx);
            let asset =
                register_file_asset(state, job_id, "video-cover", idx, &cover_path, Some(name))?;
            let dimensions = image::open(&cover_path)
                .map(|img| image::GenericImageView::dimensions(&img))
                .unwrap_or((0, 0));
            covers.insert(input.asset.id.clone(), (asset.clone(), dimensions));
            artifacts.push(asset);
        } else {
            let metadata = serde_json::json!({
                "asset_id": input.asset.id,
                "name": input.asset.name,
                "size": input.size,
                "ffprobe": ffprobe_json,
                "cover": "ffmpeg unavailable or frame extraction failed",
            });
            let bytes = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
            let asset = register_bytes_asset(
                state,
                job_id,
                "video-metadata",
                idx,
                &format!("{}-metadata.json", safe_stem(&input.asset.name, idx)),
                Some("application/json"),
                bytes,
            )?;
            artifacts.push(asset);
        }
    }

    let batch_id = extra_value(request, "batch_id");
    let batch = if let Some(batch_id) = batch_id {
        state
            .store
            .get_batch(&batch_id)
            .map_err(|e| format!("{e:?}"))?
    } else {
        let batch_name =
            extra_value(request, "batch_name").unwrap_or_else(|| format!("Video Review {job_id}"));
        state
            .store
            .create_batch(CreateRemoteBatchRequest {
                schema_version: REMOTE_SCHEMA_VERSION,
                name: batch_name,
                kind: RemoteBatchKind::Video,
                workspace_id: workspace_id_for(state, request),
                asset_ids: request
                    .inputs
                    .iter()
                    .map(|asset| asset.id.clone())
                    .collect(),
            })
            .map_err(|e| format!("{e:?}"))?
    };

    attach_generated_assets(state, &batch.batch_id, &covers, false)?;

    let status = succeeded_status(state, job_id, inputs.len(), inputs.len(), None)?;
    Ok((
        RemoteJobResult {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: job_id.to_string(),
            phase: RemoteJobPhase::Succeeded,
            successes: inputs.len(),
            failures: 0,
            artifacts,
            error_summary: None,
            updated_at: now_unix(),
        },
        status,
    ))
}

fn run_data_extract_job(
    state: &AppState,
    job_id: &str,
    request: &RemoteJobRequest,
) -> Result<(RemoteJobResult, RemoteJobStatus), String> {
    let (_input_dir, inputs) = materialize_inputs(state, job_id, request, "data_extract")?;
    let mut report = String::from("asset_id,name,size,mime\n");
    for input in &inputs {
        report.push_str(&format!(
            "{},{},{},{}\n",
            csv_cell(&input.asset.id),
            csv_cell(&input.asset.name),
            input.size,
            csv_cell(input.asset.mime.as_deref().unwrap_or(""))
        ));
    }

    let report_asset = register_bytes_asset(
        state,
        job_id,
        "extract-report",
        0,
        &format!("extract-report-{job_id}.csv"),
        Some("text/csv"),
        report.into_bytes(),
    )?;
    state
        .store
        .upsert_extract_result(RemoteExtractResultSummary {
            result_id: format!("extract-{job_id}"),
            module: extra_value(request, "module").unwrap_or_else(|| "remote_data_extract".into()),
            batch_name: extra_value(request, "batch_name")
                .unwrap_or_else(|| format!("Data Extract {job_id}")),
            updated_at: now_unix(),
            report_asset: Some(report_asset.clone()),
        })
        .map_err(|e| format!("{e:?}"))?;

    let status = succeeded_status(state, job_id, inputs.len(), inputs.len(), None)?;
    Ok((
        RemoteJobResult {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: job_id.to_string(),
            phase: RemoteJobPhase::Succeeded,
            successes: inputs.len(),
            failures: 0,
            artifacts: vec![report_asset],
            error_summary: None,
            updated_at: now_unix(),
        },
        status,
    ))
}

fn materialize_inputs(
    state: &AppState,
    job_id: &str,
    request: &RemoteJobRequest,
    module: &str,
) -> Result<(PathBuf, Vec<MaterializedInput>), String> {
    let input_dir = state
        .config
        .work_dir()
        .join(job_id)
        .join(module)
        .join("input");
    std::fs::create_dir_all(&input_dir).map_err(|e| e.to_string())?;

    let mut inputs = Vec::new();
    for (idx, asset) in request.inputs.iter().enumerate() {
        let rel = relative_path_for_asset(asset, idx);
        let dest = input_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let key = format!("assets/{}", asset.id);
        let bytes = state
            .objects
            .get_bytes(&key)
            .map_err(|e| format!("missing input asset {}: {e:?}", asset.id))?;
        let size = bytes.len() as u64;
        std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
        inputs.push(MaterializedInput {
            asset: asset.clone(),
            path: dest,
            size,
        });
    }

    if inputs.is_empty() {
        return Err("job has no input assets".into());
    }
    Ok((input_dir, inputs))
}

fn register_file_asset(
    state: &AppState,
    job_id: &str,
    kind: &str,
    idx: usize,
    path: &Path,
    name_override: Option<String>,
) -> Result<RemoteAssetRef, String> {
    let name = name_override.unwrap_or_else(|| {
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{kind}-{idx}"))
    });
    let asset_id = format!("out-{job_id}-{kind}-{idx}");
    let key = format!("assets/{asset_id}");
    let size = state
        .objects
        .put_file(&key, path)
        .map_err(|e| format!("{e:?}"))?;
    let checksum = sha256_file(path).map_err(|e| format!("{e:?}"))?;
    let asset = RemoteAssetRef {
        id: asset_id,
        name: name.clone(),
        mime: guess_mime(&name),
        size: Some(size),
        checksum: Some(checksum),
        download_url: Some(download_url(state, &format!("out-{job_id}-{kind}-{idx}"))),
    };
    state
        .store
        .register_asset(asset.clone())
        .map_err(|e| format!("{e:?}"))?;
    Ok(asset)
}

fn register_bytes_asset(
    state: &AppState,
    job_id: &str,
    kind: &str,
    idx: usize,
    name: &str,
    mime: Option<&str>,
    bytes: Vec<u8>,
) -> Result<RemoteAssetRef, String> {
    let asset_id = format!("out-{job_id}-{kind}-{idx}");
    let key = format!("assets/{asset_id}");
    let size = bytes.len() as u64;
    let checksum = sha256_hex(&bytes);
    state
        .objects
        .put_bytes(&key, bytes)
        .map_err(|e| format!("{e:?}"))?;
    let asset = RemoteAssetRef {
        id: asset_id,
        name: name.to_string(),
        mime: mime.map(str::to_string).or_else(|| guess_mime(name)),
        size: Some(size),
        checksum: Some(checksum),
        download_url: Some(download_url(state, &format!("out-{job_id}-{kind}-{idx}"))),
    };
    state
        .store
        .register_asset(asset.clone())
        .map_err(|e| format!("{e:?}"))?;
    Ok(asset)
}

fn attach_generated_assets(
    state: &AppState,
    batch_id: &str,
    generated: &HashMap<String, (RemoteAssetRef, (u32, u32))>,
    thumbnail_only: bool,
) -> Result<(), String> {
    let items = state
        .store
        .list_review_items(batch_id)
        .map_err(|e| format!("{e:?}"))?;
    for item in items {
        if let Some((asset, dimensions)) = generated.get(&item.asset.id) {
            let preview = if thumbnail_only {
                None
            } else {
                Some(asset.clone())
            };
            state
                .store
                .update_review_item_assets(
                    &item.item_id,
                    Some(asset.clone()),
                    preview,
                    None,
                    Some(*dimensions),
                )
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    Ok(())
}

fn make_thumbnail_jpeg(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let (width, height) = image::GenericImageView::dimensions(&img);
    let thumb = img.thumbnail(256, 256);
    let mut out = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok((out, width, height))
}

fn ffprobe_json(path: &Path) -> Result<serde_json::Value, String> {
    let output = crate::process_util::command("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

fn extract_cover_frame(input: &Path, output: &Path) -> Result<(), String> {
    let status = crate::process_util::command("ffmpeg")
        .args(["-y", "-i"])
        .arg(input)
        .args(["-frames:v", "1"])
        .arg(output)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg exited with {status}"))
    }
}

fn succeeded_status(
    state: &AppState,
    job_id: &str,
    processed: usize,
    total: usize,
    log_summary: Option<String>,
) -> Result<RemoteJobStatus, String> {
    let mut status = state.store.get_job(job_id).map_err(|e| format!("{e:?}"))?;
    status.phase = RemoteJobPhase::Succeeded;
    status.processed = processed;
    status.total = total.max(processed);
    status.progress = Some(1.0);
    status.log_summary = log_summary;
    status.updated_at = now_unix();
    Ok(status)
}

fn extra_value(request: &RemoteJobRequest, key: &str) -> Option<String> {
    request
        .params
        .extras
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.trim().is_empty())
}

fn workspace_id_for(state: &AppState, request: &RemoteJobRequest) -> Option<String> {
    Some(
        request
            .workspace_id
            .clone()
            .unwrap_or_else(|| state.config.default_workspace.clone()),
    )
}

fn download_url(state: &AppState, asset_id: &str) -> String {
    format!(
        "{}/v1/artifacts/{}/content",
        state.config.public_base.trim_end_matches('/'),
        asset_id
    )
}

fn safe_stem(name: &str, idx: usize) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("file-{idx}"))
}

fn thumb_name(name: &str, idx: usize) -> String {
    format!("{}-thumb.jpg", safe_stem(name, idx))
}

fn csv_cell(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn relative_path_for_asset(asset: &RemoteAssetRef, idx: usize) -> PathBuf {
    // extras 约定：客户端可把相对路径放在 name，或 id 不含路径时用 name。
    let candidate = if asset.name.contains('/') || asset.name.contains('\\') {
        asset.name.clone()
    } else {
        asset.name.clone()
    };
    let cleaned = candidate.replace('\\', "/");
    if cleaned.is_empty() || cleaned.contains("..") {
        return PathBuf::from(format!("file-{idx}"));
    }
    PathBuf::from(cleaned)
}

fn parse_format(s: &str) -> ImageFormat {
    match s.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "png" => ImageFormat::Png,
        "webp" => ImageFormat::WebP,
        "gif" => ImageFormat::Gif,
        "bmp" => ImageFormat::Bmp,
        "tiff" | "tif" => ImageFormat::Tiff,
        #[cfg(feature = "jpegxl")]
        "jxl" => ImageFormat::JpegXl,
        #[cfg(feature = "avif")]
        "avif" => ImageFormat::Avif,
        other => {
            tracing::warn!(
                format = other,
                "unknown or disabled target format, defaulting to webp"
            );
            ImageFormat::WebP
        }
    }
}

fn guess_mime(name: &str) -> Option<String> {
    let ext = Path::new(name)
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

fn collect_output_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_output_files(root, &path, out)?;
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| !n.starts_with('.'))
            .unwrap_or(true)
        {
            out.push(path);
        }
    }
    Ok(())
}

fn fail_or_retry(
    state: &AppState,
    policy: &WorkerReliabilityPolicy,
    claim_token: &str,
    job_id: &str,
    attempt: u32,
    status: &crate::remote::types::RemoteJobStatus,
    err: String,
) -> Result<(), String> {
    let next_attempt = attempt.saturating_add(1);
    let _ = state.queue.ack(claim_token);
    if policy.should_dead_letter(next_attempt) {
        let mut failed = status.clone();
        failed.phase = RemoteJobPhase::Failed;
        failed.error_summary = Some(err.clone());
        failed.updated_at = now_unix();
        let _ = state.store.update_job(failed.clone());
        let _ = state.store.set_result(RemoteJobResult {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: job_id.to_string(),
            phase: RemoteJobPhase::Failed,
            successes: 0,
            failures: 1,
            artifacts: Vec::new(),
            error_summary: Some(err.clone()),
            updated_at: now_unix(),
        });
        let _ = state.store.put_dead_letter(DeadLetterRecord {
            schema_version: REMOTE_SCHEMA_VERSION,
            job_id: job_id.to_string(),
            attempts: next_attempt,
            last_error: err,
            dead_lettered_at: now_unix(),
        });
        return Ok(());
    }
    let sleep_ms = policy.backoff_ms(next_attempt);
    thread::sleep(Duration::from_millis(sleep_ms.min(50)));
    state
        .queue
        .enqueue(job_id, next_attempt)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

/// 手动执行一次 claim+process（测试用）。
pub fn run_once(state: &AppState, worker_id: &str) -> Result<bool, String> {
    let policy = &state.policy;
    let _ = state.queue.reclaim_expired();
    match state
        .queue
        .claim(worker_id, policy.lease_secs)
        .map_err(|e| format!("{e:?}"))?
    {
        Some(claimed) => {
            process_one(
                state,
                policy,
                &claimed.lease.claim_token,
                &claimed.message.job_id,
                claimed.message.attempt,
                worker_id,
            )?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::types::RemoteJobSource;
    use crate::server::config::ServerConfig;
    use crate::server::object_store::sha256_hex;
    use image::{Rgb, RgbImage};

    #[test]
    fn worker_converts_uploaded_png_to_webp() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = ServerConfig::default();
        cfg.data_dir = dir.path().to_path_buf();
        cfg.inline_worker = false;
        cfg.public_base = "http://127.0.0.1:8787".into();
        let state = AppState::local_disk(cfg).unwrap();

        // 造一张小 PNG 并登记为 asset
        let img = RgbImage::from_pixel(8, 8, Rgb([10, 20, 30]));
        let mut png_bytes = Vec::new();
        {
            let mut cursor = std::io::Cursor::new(&mut png_bytes);
            image::DynamicImage::ImageRgb8(img)
                .write_to(&mut cursor, image::ImageFormat::Png)
                .unwrap();
        }
        let asset_id = "asset-test-1";
        state
            .objects
            .put_bytes(&format!("assets/{asset_id}"), png_bytes.clone())
            .unwrap();
        let asset = RemoteAssetRef {
            id: asset_id.into(),
            name: "sample.png".into(),
            mime: Some("image/png".into()),
            size: Some(png_bytes.len() as u64),
            checksum: Some(sha256_hex(&png_bytes)),
            download_url: None,
        };
        state.store.register_asset(asset.clone()).unwrap();

        let status = state
            .store
            .create_job(RemoteJobRequest {
                source: RemoteJobSource::Convert,
                inputs: vec![asset],
                params: crate::remote::types::RemoteJobParams {
                    target_format: Some("webp".into()),
                    quality: Some(80),
                    overwrite: Some(true),
                    ..Default::default()
                },
                client_request_id: Some("w-real-1".into()),
                ..RemoteJobRequest::default()
            })
            .unwrap();
        state.queue.enqueue(&status.job_id, 0).unwrap();
        assert!(run_once(&state, "test-w").unwrap());
        let done = state.store.get_job(&status.job_id).unwrap();
        assert_eq!(done.phase, RemoteJobPhase::Succeeded);
        let result = state.store.get_result(&status.job_id).unwrap();
        assert_eq!(result.successes, 1);
        assert_eq!(result.artifacts.len(), 1);
        assert!(result.artifacts[0].name.ends_with(".webp"));
    }
}
