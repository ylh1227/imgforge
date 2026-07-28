//! Host 方法分发。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::AppConfig;
use crate::core::types::{
    BrightnessMatchMetric, BrightnessMatchMode, BrightnessMatchOptions, ImageFormat,
    MetadataPolicy, Quality,
};
use crate::host::protocol::{HostEvent, RpcError, RpcId, RpcRequest, RpcResponse};
use crate::host::state::{EventSink, HostState};
use crate::job::{preview_batch, run_batch};
use crate::mobile::{list_devices, MobilePullConfig};
use crate::prefs::{ConvertPresetSnapshot, GuiPrefs, TaskHistoryEntry};
use crate::review::domain::image_item::{ImageFilter, ReviewStatus};
use crate::ui::doctor::doctor_report;
use crate::ui::progress::ProgressReporter;
use crate::video_review::service::ImportFolderOptions;

pub fn dispatch(
    state: &mut HostState,
    request: RpcRequest,
    events: Option<EventSink>,
) -> RpcResponse {
    let id = request.id.clone();
    match handle(state, &request.method, request.params, events) {
        Ok(result) => RpcResponse::result(id, result),
        Err(err) => RpcResponse::error(id, err),
    }
}

fn handle(
    state: &mut HostState,
    method: &str,
    params: Value,
    events: Option<EventSink>,
) -> Result<Value, RpcError> {
    match method {
        "app.ping" => Ok(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") })),
        "app.doctor" => serde_json::to_value(doctor_report()).map_err(ser_err),
        "app.open_path" => {
            let path: String = required(&params, "path")?;
            open::that(&path).map_err(|e| RpcError::app(e.to_string()))?;
            Ok(json!({ "ok": true }))
        }
        "app.cancel_job" => {
            let job_id: String = required(&params, "job_id")?;
            Ok(json!({ "cancelled": state.cancel_job(&job_id) }))
        }
        "app.formats" => {
            let formats: Vec<_> = ImageFormat::all_supported()
                .into_iter()
                .map(|f| {
                    json!({
                        "id": f.extension(),
                        "extension": f.extension(),
                        "mime": f.mime_type(),
                    })
                })
                .collect();
            Ok(json!({ "formats": formats }))
        }

        "prefs.get" => serde_json::to_value(&state.prefs).map_err(ser_err),
        "prefs.set" => {
            let prefs: GuiPrefs = serde_json::from_value(params)
                .map_err(|e| RpcError::invalid_params(e.to_string()))?;
            state.prefs = prefs;
            state.save_prefs().map_err(RpcError::app)?;
            Ok(json!({ "ok": true }))
        }
        "prefs.upsert_preset" => {
            #[derive(Deserialize)]
            struct P {
                name: String,
                snapshot: ConvertPresetSnapshot,
            }
            let p: P = parse(params)?;
            state.prefs.upsert_preset(p.name, p.snapshot);
            state.save_prefs().map_err(RpcError::app)?;
            Ok(json!({ "ok": true }))
        }
        "prefs.delete_preset" => {
            let name: String = required(&params, "name")?;
            state.prefs.delete_preset(&name);
            state.save_prefs().map_err(RpcError::app)?;
            Ok(json!({ "ok": true }))
        }

        "tasks.history" => Ok(json!({
            "convert": state.prefs.history,
            "actions": state.prefs.action_history,
        })),
        "tasks.clear_convert_history" => {
            state.prefs.history.clear();
            state.save_prefs().map_err(RpcError::app)?;
            Ok(json!({ "ok": true }))
        }

        "convert.preview" => {
            let config = convert_config_from_params(state, &params)?;
            let preview = preview_batch(&config).map_err(|e| RpcError::app(e.to_string()))?;
            Ok(json!({
                "matched": preview.matched,
                "to_convert": preview.to_convert,
                "skipped_existing": preview.skipped_existing,
                "output_conflicts": preview.output_conflicts,
                "samples": preview.samples.iter().map(|s| json!({
                    "input": s.input,
                    "output": s.output,
                })).collect::<Vec<_>>(),
                "conflict_examples": preview.conflict_examples,
            }))
        }
        "convert.run" => convert_run(state, params, events),

        "mobile.list_devices" => {
            let mut cfg = MobilePullConfig::default();
            if let Some(path) = params.get("adb_path").and_then(|v| v.as_str()) {
                cfg.adb_path = Some(PathBuf::from(path));
            }
            let devices = list_devices(&cfg).map_err(|e| RpcError::app(e.to_string()))?;
            serde_json::to_value(devices).map_err(ser_err)
        }

        "remote.status" => {
            let mut remote = crate::remote::RemoteConfig::default();
            remote.apply_env_overrides();
            Ok(json!({
                "status": remote.status_label(),
                "enabled": remote.enabled,
                "configured": remote.is_configured(),
                "base_url": remote.base_url,
                "prefer_remote": state.prefer_remote,
            }))
        }
        "remote.set_prefer" => {
            state.prefer_remote = params
                .get("prefer")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Ok(json!({ "prefer_remote": state.prefer_remote }))
        }

        "jira.status" => {
            let jira = crate::jira::load_jira_config_with_prefs(Some(&state.prefs.jira));
            Ok(json!({
                "status": jira.status_label(),
                "enabled": jira.enabled,
                "base_url": jira.base_url,
                "project_key": jira.project_key,
                "has_credentials": jira.has_credentials(),
            }))
        }
        "jira.probe" => {
            let jira = crate::jira::load_jira_config_with_prefs(Some(&state.prefs.jira));
            match crate::jira::client::JiraClient::probe(&jira) {
                Ok(me) => Ok(json!({
                    "ok": true,
                    "display_name": me.display_name,
                })),
                Err(e) => Ok(json!({ "ok": false, "message": e.to_string() })),
            }
        }

        "review.list_batches" => {
            let batches = state
                .review()?
                .batch_service()
                .list_batches()
                .map_err(app_err)?;
            serde_json::to_value(batches).map_err(ser_err)
        }
        "review.batch_stats" => {
            let batch_id: i64 = required(&params, "batch_id")?;
            let stats = state
                .review()?
                .batch_service()
                .batch_stats(batch_id)
                .map_err(app_err)?;
            serde_json::to_value(stats).map_err(ser_err)
        }
        "review.import_folder" => {
            let folder: String = required(&params, "folder")?;
            let name: String = optional_string(&params, "name")
                .unwrap_or_else(|| default_batch_name(&folder));
            let recursive = params
                .get("recursive")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let id = state
                .review()?
                .batch_service()
                .create_from_folder(&name, Path::new(&folder), recursive)
                .map_err(app_err)?;
            Ok(json!({ "batch_id": id }))
        }
        "review.import_paths" => {
            let name: String = optional_string(&params, "name").unwrap_or_else(|| "队列导入".into());
            let paths: Vec<String> = required(&params, "paths")?;
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            let id = state
                .review()?
                .batch_service()
                .create_from_paths(&name, &paths)
                .map_err(app_err)?;
            Ok(json!({ "batch_id": id }))
        }
        "review.list_images" => {
            let batch_id: i64 = required(&params, "batch_id")?;
            let filter = ImageFilter::default();
            let items = state
                .review()?
                .list_images(batch_id, &filter)
                .map_err(app_err)?;
            serde_json::to_value(items).map_err(ser_err)
        }
        "review.set_status" => {
            let image_id: i64 = required(&params, "image_id")?;
            let status: ReviewStatus = required(&params, "status")?;
            state.review()?.set_status(image_id, status).map_err(app_err)?;
            Ok(json!({ "ok": true }))
        }
        "review.set_remark" => {
            let image_id: i64 = required(&params, "image_id")?;
            let remark: String = required(&params, "remark")?;
            state
                .review()?
                .set_remark(image_id, &remark)
                .map_err(app_err)?;
            Ok(json!({ "ok": true }))
        }
        "review.load_annotations" => {
            let image_id: i64 = required(&params, "image_id")?;
            let anns = state
                .review()?
                .load_annotations(image_id)
                .map_err(app_err)?;
            serde_json::to_value(anns).map_err(ser_err)
        }
        "review.add_annotation" => {
            let ann = serde_json::from_value(params)
                .map_err(|e| RpcError::invalid_params(e.to_string()))?;
            let id = state.review()?.add_annotation(&ann).map_err(app_err)?;
            Ok(json!({ "id": id }))
        }
        "review.delete_annotation" => {
            let id: i64 = required(&params, "id")?;
            state.review()?.remove_annotation(id).map_err(app_err)?;
            Ok(json!({ "ok": true }))
        }
        "review.session_restore" => {
            let (batch_id, image_id) = state.review()?.restore_session().map_err(app_err)?;
            Ok(json!({ "batch_id": batch_id, "image_id": image_id }))
        }
        "review.session_save" => {
            let batch_id: i64 = required(&params, "batch_id")?;
            let image_id: i64 = required(&params, "image_id")?;
            state
                .review()?
                .save_session(batch_id, image_id)
                .map_err(app_err)?;
            Ok(json!({ "ok": true }))
        }
        "review.export_csv" => {
            let batch_id: i64 = required(&params, "batch_id")?;
            let path: String = required(&params, "path")?;
            state
                .review()?
                .export_csv(batch_id, Path::new(&path))
                .map_err(app_err)?;
            Ok(json!({ "ok": true, "path": path }))
        }

        "video.availability" => {
            let avail = state.video()?.availability();
            Ok(json!({
                "ffmpeg_ok": avail.ffmpeg_ok,
                "ffprobe_ok": avail.ffprobe_ok,
                "ffmpeg_version": avail.ffmpeg_version,
                "ffprobe_version": avail.ffprobe_version,
            }))
        }
        "video.list_batches" => {
            let batches = state.video()?.list_batches().map_err(app_err)?;
            serde_json::to_value(batches).map_err(ser_err)
        }
        "video.batch_stats" => {
            let batch_id: i64 = required(&params, "batch_id")?;
            let stats = state.video()?.batch_stats(batch_id).map_err(app_err)?;
            serde_json::to_value(stats).map_err(ser_err)
        }
        "video.import_folder" => {
            let folder: String = required(&params, "folder")?;
            let batch_name = optional_string(&params, "name");
            let generate_thumbnails = params
                .get("generate_thumbnails")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let result = state
                .video()?
                .import_folder_with_options(
                    Path::new(&folder),
                    batch_name.as_deref(),
                    ImportFolderOptions { generate_thumbnails },
                    None,
                )
                .map_err(app_err)?;
            Ok(json!({
                "batch_id": result.batch_id,
                "imported": result.imported,
                "skipped": result.skipped.iter().map(|s| json!({
                    "path": s.path,
                    "reason": s.reason,
                })).collect::<Vec<_>>(),
            }))
        }
        "video.list_videos" => {
            let batch_id: i64 = required(&params, "batch_id")?;
            let filter = crate::video_review::domain::VideoFilter::default();
            let items = state
                .video()?
                .list_videos(batch_id, &filter)
                .map_err(app_err)?;
            serde_json::to_value(items).map_err(ser_err)
        }
        "video.get" => {
            let id: i64 = required(&params, "id")?;
            let item = state.video()?.get_video(id).map_err(app_err)?;
            serde_json::to_value(item).map_err(ser_err)
        }
        "video.set_status" => {
            let id: i64 = required(&params, "id")?;
            let status: ReviewStatus = required(&params, "status")?;
            state.video()?.update_status(id, status).map_err(app_err)?;
            Ok(json!({ "ok": true }))
        }
        "video.set_remark" => {
            let id: i64 = required(&params, "id")?;
            let remark: String = required(&params, "remark")?;
            state
                .video()?
                .update_remark(id, &remark)
                .map_err(app_err)?;
            Ok(json!({ "ok": true }))
        }
        "video.set_offset" => {
            let id: i64 = required(&params, "id")?;
            let offset_ms: i64 = required(&params, "offset_ms")?;
            state
                .video()?
                .update_offset(id, offset_ms)
                .map_err(app_err)?;
            Ok(json!({ "ok": true }))
        }
        "video.frame_at" => {
            let id: i64 = required(&params, "id")?;
            let pts_ms: u64 = required(&params, "pts_ms")?;
            let width = params
                .get("width")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(960);
            let item = state.video()?.get_video(id).map_err(app_err)?;
            let path = state
                .video()?
                .frame_at(&item, pts_ms, width)
                .map_err(app_err)?;
            Ok(json!({ "path": path }))
        }
        "video.ensure_cover" => {
            let id: i64 = required(&params, "id")?;
            let item = state.video()?.get_video(id).map_err(app_err)?;
            let path = state.video()?.ensure_cover(&item).map_err(app_err)?;
            Ok(json!({ "path": path }))
        }
        "video.list_tags" => {
            let tags = state.video()?.list_tags().map_err(app_err)?;
            serde_json::to_value(tags).map_err(ser_err)
        }
        "video.list_markers" => {
            let video_id: i64 = required(&params, "video_id")?;
            let markers = state.video()?.list_markers(video_id).map_err(app_err)?;
            serde_json::to_value(markers).map_err(ser_err)
        }
        "video.list_segments" => {
            let video_id: i64 = required(&params, "video_id")?;
            let segments = state.video()?.list_segments(video_id).map_err(app_err)?;
            serde_json::to_value(segments).map_err(ser_err)
        }
        "video.align" => {
            let ids: Vec<i64> = required(&params, "ids")?;
            if ids.is_empty() {
                return Err(RpcError::invalid_params("ids required"));
            }
            let quality = match params
                .get("quality")
                .and_then(|v| v.as_str())
                .unwrap_or("fast")
            {
                "standard" => crate::video_review::service::AlignQuality::Standard,
                "fine" => crate::video_review::service::AlignQuality::Fine,
                _ => crate::video_review::service::AlignQuality::Fast,
            };
            let mode = crate::video_review::service::AlignMode::Auto;
            let svc = state.video()?;
            let reference = svc.get_video(ids[0]).map_err(app_err)?;
            let mut others = Vec::new();
            for id in ids.iter().skip(1) {
                others.push(svc.get_video(*id).map_err(app_err)?);
            }
            let result = svc
                .align_videos(&reference, &others, None, mode, quality, None)
                .map_err(app_err)?;
            Ok(json!({
                "reference_id": result.reference_id,
                "elapsed_ms": result.elapsed_ms,
                "pairs": result.pairs.iter().map(|p| json!({
                    "video_id": p.video_id,
                    "offset_ms": p.offset_ms,
                    "confidence": p.confidence,
                    "method": p.method,
                    "drift_ppm": p.drift_ppm,
                })).collect::<Vec<_>>(),
            }))
        }
        "video.export_contact_sheet" => {
            let ids: Vec<i64> = required(&params, "ids")?;
            let pts_ms: u64 = required(&params, "pts_ms")?;
            let output: String = required(&params, "output")?;
            let svc = state.video()?;
            let mut videos = Vec::new();
            for id in ids {
                videos.push(svc.get_video(id).map_err(app_err)?);
            }
            let result = svc
                .export_compare_contact_sheet(&videos, pts_ms, PathBuf::from(&output))
                .map_err(app_err)?;
            Ok(json!({
                "path": result.dest,
                "cols": result.cols,
                "rows": result.rows,
                "width": result.width,
                "height": result.height,
                "video_count": result.video_count,
            }))
        }
        "video.frame_cache_stats" => {
            let stats = state.video()?.frame_cache_stats().map_err(app_err)?;
            Ok(json!({
                "file_count": stats.file_count,
                "total_bytes": stats.total_bytes,
                "pending_count": stats.pending_count,
            }))
        }
        "video.clear_frame_cache" => {
            let n = state.video()?.clear_frame_cache().map_err(app_err)?;
            Ok(json!({ "removed": n }))
        }
        "video.batch_update_status" => {
            let ids: Vec<i64> = required(&params, "ids")?;
            let status: ReviewStatus = required(&params, "status")?;
            let result = state
                .video()?
                .batch_update_status_result(&ids, status);
            Ok(json!({
                "requested": result.requested,
                "applied": result.applied,
                "failed": result.failed,
                "failures": result.failures,
            }))
        }
        "video.export_grid_video" => {
            let ids: Vec<i64> = required(&params, "ids")?;
            let start_ms: u64 = params
                .get("start_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let duration_ms: u64 = params
                .get("duration_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(5000);
            let output: String = required(&params, "output")?;
            let quality = match params
                .get("quality")
                .and_then(|v| v.as_str())
                .unwrap_or("high")
            {
                "lossless" => {
                    crate::video_review::service::grid_video::GridVideoExportQuality::Lossless
                }
                _ => crate::video_review::service::grid_video::GridVideoExportQuality::High,
            };
            let svc = state.video()?;
            let mut videos = Vec::new();
            for id in ids {
                videos.push(svc.get_video(id).map_err(app_err)?);
            }
            let result = svc
                .export_compare_grid_video(
                    &videos,
                    start_ms,
                    duration_ms,
                    PathBuf::from(&output),
                    quality,
                    crate::video_review::service::grid_video::GridVideoCaptionMode::default(),
                )
                .map_err(app_err)?;
            Ok(json!({
                "path": result.dest,
                "width": result.width,
                "height": result.height,
                "duration_ms": result.duration_ms,
            }))
        }

        "extract.scan" => {
            let root: String = required(&params, "root")?;
            let files = crate::data_extract::service::scanner::scan_directory(Path::new(&root));
            Ok(json!({ "files": files }))
        }
        "extract.from_path" => {
            let path: String = required(&params, "path")?;
            let batch = crate::data_extract::service::DataExtractService::extract_from_path(
                Path::new(&path),
            )
            .map_err(app_err)?;
            serde_json::to_value(batch).map_err(ser_err)
        }
        "extract.summary" => {
            let path: String = required(&params, "path")?;
            let batch = crate::data_extract::service::DataExtractService::extract_from_path(
                Path::new(&path),
            )
            .map_err(app_err)?;
            let summary =
                crate::data_extract::service::SummaryService::build(std::slice::from_ref(&batch));
            serde_json::to_value(summary).map_err(ser_err)
        }

        other => Err(RpcError::method_not_found(other)),
    }
}

fn convert_run(
    state: &mut HostState,
    params: Value,
    events: Option<EventSink>,
) -> Result<Value, RpcError> {
    let config = convert_config_from_params(state, &params)?;
    let snapshot = snapshot_from_config(&config);
    let (job_id, cancel, progress) = state.begin_job("convert");
    let events = events.clone();
    let job_id_bg = job_id.clone();
    let progress_bg = Arc::clone(&progress);
    let cancel_bg = Arc::clone(&cancel);

    // 同步跑完再返回（Flutter 也可轮询 events）；长任务会阻塞一行 RPC，符合 stdio 请求模型。
    // 若 params.async == true，则后台跑并立即返回 job_id。
    let async_mode = params
        .get("async")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if async_mode {
        let input_dir = config.input_dir.display().to_string();
        let output_dir = config.output_dir.display().to_string();
        thread::spawn(move || {
            emit_progress(&events, &job_id_bg, &progress_bg, "starting");
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    emit_finished(&events, &job_id_bg, false, e.to_string(), None);
                    return;
                }
            };
            let progress_reporter: Arc<dyn ProgressReporter> = progress_bg.clone();
            let result = rt.block_on(run_batch(
                config,
                cancel_bg,
                Some(progress_reporter),
            ));
            match result {
                Ok(report) => {
                    let msg = format!(
                        "done {}/{} in {}ms",
                        report.successes,
                        report.total,
                        report.elapsed.as_millis()
                    );
                    emit_finished(
                        &events,
                        &job_id_bg,
                        true,
                        msg,
                        Some(json!({
                            "successes": report.successes,
                            "failures": report.failures.len(),
                            "total": report.total,
                            "scanned": report.scanned,
                            "skipped": report.skipped,
                            "elapsed_ms": report.elapsed.as_millis() as u64,
                            "cancelled": report.cancelled,
                        })),
                    );
                    let _ = (input_dir, output_dir, snapshot);
                }
                Err(e) => emit_finished(&events, &job_id_bg, false, e.to_string(), None),
            }
        });
        return Ok(json!({ "job_id": job_id, "async": true }));
    }

    let progress_reporter: Arc<dyn ProgressReporter> = progress.clone();
    let rt = tokio::runtime::Runtime::new().map_err(|e| RpcError::internal(e.to_string()))?;
    let result = rt.block_on(run_batch(config.clone(), cancel, Some(progress_reporter)));
    state.finish_job(&job_id);
    match result {
        Ok(report) => {
            state.prefs.push_history(TaskHistoryEntry {
                finished_at_unix: crate::prefs::now_unix(),
                input_dir: config.input_dir.display().to_string(),
                output_dir: config.output_dir.display().to_string(),
                successes: report.successes,
                failures: report.failures.len(),
                total: report.total,
                elapsed_ms: report.elapsed.as_millis() as u64,
                snapshot,
            });
            let _ = state.save_prefs();
            Ok(json!({
                "successes": report.successes,
                "failures": report.failures.len(),
                "total": report.total,
                "scanned": report.scanned,
                "skipped": report.skipped,
                "elapsed_ms": report.elapsed.as_millis() as u64,
                "cancelled": report.cancelled,
            }))
        }
        Err(e) => Err(RpcError::app(e.to_string())),
    }
}

fn emit_progress(events: &Option<EventSink>, job_id: &str, progress: &crate::ui::progress::GuiProgress, message: &str) {
    if let Some(sink) = events {
        let total = progress.total.load(Ordering::Relaxed);
        let current = progress.completed.load(Ordering::Relaxed);
        sink(HostEvent::JobProgress {
            job_id: job_id.into(),
            current,
            total,
            fraction: progress.fraction(),
            message: message.into(),
        });
    }
}

fn emit_finished(
    events: &Option<EventSink>,
    job_id: &str,
    ok: bool,
    message: String,
    result: Option<Value>,
) {
    if let Some(sink) = events {
        sink(HostEvent::JobFinished {
            job_id: job_id.into(),
            ok,
            message,
            result,
        });
    }
}

fn convert_config_from_params(state: &HostState, params: &Value) -> Result<AppConfig, RpcError> {
    let mut config = AppConfig::default();
    config.input_dir = PathBuf::from(required::<String>(params, "input_dir")?);
    config.output_dir = PathBuf::from(
        optional_string(params, "output_dir").unwrap_or_else(|| "./output".into()),
    );
    if let Some(fmt) = optional_string(params, "format") {
        config.target_format = parse_format(&fmt)?;
    }
    if let Some(q) = params.get("quality").and_then(|v| v.as_u64()) {
        config.quality = Quality::new(q as u8).map_err(|e| RpcError::invalid_params(e.to_string()))?;
    }
    config.recursive = params
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    config.preserve_structure = params
        .get("preserve_structure")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    config.overwrite = params
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    config.bayer_only = params
        .get("bayer_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if params
        .get("strip_metadata")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        config.metadata_policy = MetadataPolicy::Strip;
    }
    if let Some(t) = optional_string(params, "rename_template") {
        if !t.is_empty() {
            config.rename_template = Some(t);
        }
    }
    if let Some(kb) = params.get("target_max_kb").and_then(|v| v.as_u64()) {
        if kb > 0 {
            config.target_max_bytes = Some(kb.saturating_mul(1024));
        }
    }
    if params
        .get("brightness_match_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let mode = optional_string(params, "brightness_match_mode").unwrap_or_default();
        let mode = match mode.as_str() {
            "global" | "Global" | "reference" | "Reference" => BrightnessMatchMode::Global,
            _ => BrightnessMatchMode::Paired,
        };
        config.brightness_match = BrightnessMatchOptions {
            enabled: true,
            mode,
            reference_path: optional_string(params, "brightness_match_path").map(PathBuf::from),
            metric: if params
                .get("brightness_match_metric_percentile")
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
            {
                BrightnessMatchMetric::Percentile
            } else {
                BrightnessMatchMetric::Mean
            },
            percentile: params
                .get("brightness_match_percentile")
                .and_then(|v| v.as_f64())
                .unwrap_or(98.0) as f32,
            regional: params
                .get("brightness_match_regional")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            grid_cols: 3,
            grid_rows: 3,
        };
    }
    config.burn_review_annotations = state.burn_review_annotations;
    if let Some(paths) = params.get("explicit_inputs").and_then(|v| v.as_array()) {
        config.explicit_inputs = paths
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect();
    }
    Ok(config)
}

fn snapshot_from_config(config: &AppConfig) -> ConvertPresetSnapshot {
    ConvertPresetSnapshot {
        format: config.target_format,
        quality: config.quality.value(),
        resize: config.resize.clone(),
        recursive: config.recursive,
        preserve_structure: config.preserve_structure,
        overwrite: config.overwrite,
        strip_metadata: matches!(config.metadata_policy, MetadataPolicy::Strip),
        bayer_only: config.bayer_only,
        rename_template: config.rename_template.clone().unwrap_or_default(),
        target_max_bytes: config.target_max_bytes,
        use_target_max_bytes: config.target_max_bytes.is_some(),
        brightness_match_enabled: config.brightness_match.enabled,
        brightness_match_mode: config.brightness_match.mode,
        brightness_match_path: config
            .brightness_match
            .reference_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        brightness_match_metric_percentile: matches!(
            config.brightness_match.metric,
            BrightnessMatchMetric::Percentile
        ),
        brightness_match_percentile: config.brightness_match.percentile,
        brightness_match_regional: config.brightness_match.regional,
    }
}

fn parse_format(s: &str) -> Result<ImageFormat, RpcError> {
    ImageFormat::from_extension(s)
        .or_else(|| match s.to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => Some(ImageFormat::Jpeg),
            "png" => Some(ImageFormat::Png),
            "webp" => Some(ImageFormat::WebP),
            "bmp" => Some(ImageFormat::Bmp),
            "tiff" | "tif" => Some(ImageFormat::Tiff),
            "gif" => Some(ImageFormat::Gif),
            #[cfg(feature = "jpegxl")]
            "jxl" | "jpegxl" => Some(ImageFormat::JpegXl),
            _ => None,
        })
        .ok_or_else(|| RpcError::invalid_params(format!("unknown format: {s}")))
}

fn default_batch_name(folder: &str) -> String {
    Path::new(folder)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("batch")
        .to_string()
}

fn required<T: for<'de> Deserialize<'de>>(params: &Value, key: &str) -> Result<T, RpcError> {
    let value = params
        .get(key)
        .ok_or_else(|| RpcError::invalid_params(format!("missing param: {key}")))?;
    serde_json::from_value(value.clone()).map_err(|e| RpcError::invalid_params(e.to_string()))
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))
}

fn ser_err(e: serde_json::Error) -> RpcError {
    RpcError::internal(e.to_string())
}

fn app_err(e: impl std::fmt::Display) -> RpcError {
    RpcError::app(e.to_string())
}

impl From<String> for RpcError {
    fn from(value: String) -> Self {
        RpcError::app(value)
    }
}
