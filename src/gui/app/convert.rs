//! 转换配置、预览、worker 与设置控件。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use eframe::egui;

use crate::config::AppConfig;
use crate::core::types::{
    BrightnessMatchMetric, BrightnessMatchMode, BrightnessMatchOptions, ImageFormat, MetadataPolicy,
    Quality,
    ResizeOptions,
};
use crate::gui::app_types::{AppMode, RunState, WorkerMessage};
use crate::gui::prefs::{self, ConvertPresetSnapshot, TaskHistoryEntry};
use crate::gui::quality_preview::{self, QualityPreviewWorker};
use crate::gui::{theme, widgets};
use crate::job::{preview_batch, run_batch};
use crate::ui::progress::{GuiProgress, ProgressReporter};
use crate::ui::report::ProcessReport;

use super::ImgforgeApp;

impl ImgforgeApp {
    pub(super) fn build_config(&self) -> Result<AppConfig, String> {
        let input = PathBuf::from(&self.input_dir);
        if self.input_dir.trim().is_empty() {
            return Err("请选择输入文件夹".into());
        }
        if !input.exists() {
            return Err(format!("输入文件夹不存在：{}", input.display()));
        }

        let output = PathBuf::from(&self.output_dir);
        if self.output_dir.trim().is_empty() {
            return Err("请指定输出文件夹".into());
        }

        let target_format = self.formats[self.format_index];
        let quality = if self.use_target_max_bytes {
            Quality::DEFAULT
        } else {
            Quality::new(self.quality).map_err(|e| e.to_string())?
        };

        let mut config = AppConfig::default();
        config.input_dir = input;
        config.output_dir = output;
        config.target_format = target_format;
        config.quality = quality;
        config.recursive = self.recursive;
        config.preserve_structure = self.preserve_structure;
        config.overwrite = self.overwrite;
        config.metadata_policy = if self.strip_metadata {
            MetadataPolicy::Strip
        } else {
            MetadataPolicy::Preserve
        };
        if !self.rename_template.trim().is_empty() {
            config.rename_template = Some(self.rename_template.trim().to_string());
        }
        if self.use_target_max_bytes {
            config.target_max_bytes = Some(self.target_max_kb as u64 * 1024);
        }
        if !self.review_queue.is_empty() {
            config.explicit_inputs = self.review_queue.clone();
            if let Some(parent) = self.review_queue[0].parent() {
                config.input_dir = parent.to_path_buf();
            }
            config.per_input_params = self.review_queue_params.clone();
        }
        config.burn_review_annotations = self.burn_review_annotations;
        config.bayer_only = self.bayer_only;
        config.brightness_match = BrightnessMatchOptions {
            enabled: match self.brightness_match_mode {
                BrightnessMatchMode::Global => {
                    self.brightness_match_enabled && !self.brightness_match_path.trim().is_empty()
                }
                BrightnessMatchMode::Paired => self.brightness_match_enabled,
            },
            mode: self.brightness_match_mode,
            reference_path: {
                let p = self.brightness_match_path.trim();
                if p.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(p))
                }
            },
            metric: if self.brightness_match_metric_percentile {
                BrightnessMatchMetric::Percentile
            } else {
                BrightnessMatchMetric::Mean
            },
            percentile: self.brightness_match_percentile.clamp(0.0, 100.0),
            regional: self.brightness_match_regional,
            grid_cols: 3,
            grid_rows: 3,
        };
        config.remote = self.remote_config.clone();
        config.validate().map_err(|e| e.to_string())?;
        Ok(config)
    }

    pub(super) fn snapshot_from_ui(&self) -> ConvertPresetSnapshot {
        ConvertPresetSnapshot {
            format: self.formats[self.format_index],
            quality: self.quality,
            resize: ResizeOptions {
                width: None,
                height: None,
                mode: crate::core::types::ResizeMode::Fit,
            },
            recursive: self.recursive,
            preserve_structure: self.preserve_structure,
            overwrite: self.overwrite,
            strip_metadata: self.strip_metadata,
            bayer_only: self.bayer_only,
            rename_template: self.rename_template.clone(),
            target_max_bytes: if self.use_target_max_bytes {
                Some(self.target_max_kb as u64 * 1024)
            } else {
                None
            },
            use_target_max_bytes: self.use_target_max_bytes,
            brightness_match_enabled: self.brightness_match_enabled,
            brightness_match_mode: self.brightness_match_mode,
            brightness_match_path: self.brightness_match_path.clone(),
            brightness_match_metric_percentile: self.brightness_match_metric_percentile,
            brightness_match_percentile: self.brightness_match_percentile,
            brightness_match_regional: self.brightness_match_regional,
        }
    }

    pub(super) fn apply_snapshot(&mut self, snapshot: &ConvertPresetSnapshot) {
        if let Some(idx) = self.formats.iter().position(|f| *f == snapshot.format) {
            self.format_index = idx;
        }
        self.quality = snapshot.quality;
        self.recursive = snapshot.recursive;
        self.preserve_structure = snapshot.preserve_structure;
        self.overwrite = snapshot.overwrite;
        self.strip_metadata = snapshot.strip_metadata;
        self.bayer_only = snapshot.bayer_only;
        self.rename_template = snapshot.rename_template.clone();
        self.use_target_max_bytes = snapshot.use_target_max_bytes;
        if let Some(bytes) = snapshot.target_max_bytes {
            self.target_max_kb = (bytes / 1024).max(1) as u32;
        }
        self.brightness_match_enabled = snapshot.brightness_match_enabled;
        self.brightness_match_mode = snapshot.brightness_match_mode;
        self.brightness_match_path = snapshot.brightness_match_path.clone();
        self.brightness_match_metric_percentile = snapshot.brightness_match_metric_percentile;
        self.brightness_match_percentile = snapshot.brightness_match_percentile.clamp(90.0, 99.0);
        self.brightness_match_regional = snapshot.brightness_match_regional;
        self.brightness_match_preview = None;
        self.refresh_previews();
    }

    pub(super) fn persist_brightness_match_prefs(&mut self) {
        self.gui_prefs.brightness_match = prefs::BrightnessMatchPrefs {
            enabled: self.brightness_match_enabled,
            mode: self.brightness_match_mode,
            path: self.brightness_match_path.clone(),
            metric_percentile: self.brightness_match_metric_percentile,
            percentile: self.brightness_match_percentile,
            regional: self.brightness_match_regional,
        };
        let _ = self.gui_prefs.save();
    }

    pub(super) fn set_brightness_match_path(&mut self, path: PathBuf) {
        self.brightness_match_path = path.display().to_string();
        self.brightness_match_enabled = true;
        self.brightness_match_mode = BrightnessMatchMode::Global;
        self.brightness_match_preview = None;
        self.persist_brightness_match_prefs();
    }

    pub(super) fn clear_brightness_match_path(&mut self) {
        self.brightness_match_path.clear();
        self.brightness_match_preview = None;
        if self.brightness_match_mode == BrightnessMatchMode::Global {
            self.brightness_match_enabled = false;
        }
        self.persist_brightness_match_prefs();
    }

    pub(super) fn pick_reference_from_input_dir(&mut self) {
        let input = PathBuf::from(self.input_dir.trim());
        if self.input_dir.trim().is_empty() || !input.is_dir() {
            self.status = "请先选择输入文件夹".into();
            return;
        }
        match crate::io::reference_pick::pick_reference_from_input(&input, self.recursive) {
            Some(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                self.set_brightness_match_path(path);
                self.status = format!("已从输入目录选择参考图：{name}");
            }
            None => {
                self.status = "未找到可用参考图（jpg/jpeg/png/webp）".into();
            }
        }
    }

    fn brightness_match_path_issue(&self) -> Option<String> {
        if !self.brightness_match_enabled
            || self.brightness_match_mode != BrightnessMatchMode::Global
        {
            return None;
        }
        let p = self.brightness_match_path.trim();
        if p.is_empty() {
            return Some("已启用但未选择参考图".into());
        }
        let path = PathBuf::from(p);
        if !path.exists() {
            return Some("参考图文件不存在".into());
        }
        if !path.is_file() {
            return Some("参考图路径不是文件".into());
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !crate::io::reference_pick::is_reference_image_ext(ext) {
            return Some("格式仅支持 jpg/jpeg/png/webp".into());
        }
        None
    }

    fn ensure_brightness_match_preview(&mut self, ctx: &egui::Context) {
        let path = self.brightness_match_path.trim().to_string();
        if path.is_empty() {
            self.brightness_match_preview = None;
            return;
        }
        if self
            .brightness_match_preview
            .as_ref()
            .is_some_and(|(p, _)| p == &path)
        {
            return;
        }
        self.brightness_match_preview = None;
        let Ok(img) = image::open(&path) else {
            return;
        };
        let thumb = img.thumbnail(48, 48).to_rgba8();
        let size = [thumb.width() as usize, thumb.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw());
        let tex = ctx.load_texture(
            "brightness_match_ref_preview",
            color,
            egui::TextureOptions::LINEAR,
        );
        self.brightness_match_preview = Some((path, tex));
    }

    pub(super) fn refresh_previews(&mut self) {
        self.batch_preview = None;
        self.rename_preview.clear();
        self.rename_preview_error = None;

        if let Ok(config) = self.build_config() {
            if let Ok(preview) = preview_batch(&config) {
                self.batch_preview = Some(preview);
            }
        }

        if !self.rename_template.trim().is_empty() && !self.input_dir.trim().is_empty() {
            let input = PathBuf::from(&self.input_dir);
            let output = PathBuf::from(&self.output_dir);
            if input.exists() {
                match crate::io::batch_preview::rename_preview_samples(
                    &input,
                    &output,
                    self.rename_template.trim(),
                    self.formats[self.format_index],
                    self.preserve_structure,
                    self.recursive,
                    5,
                ) {
                    Ok(samples) => {
                        self.rename_preview = samples
                            .into_iter()
                            .map(|(path, name)| {
                                let stem = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?")
                                    .to_string();
                                let out = name.unwrap_or_else(|e| format!("错误: {e}"));
                                (stem, out)
                            })
                            .collect();
                    }
                    Err(e) => self.rename_preview_error = Some(e.to_string()),
                }
            }
        }
    }

    pub(super) fn request_quality_preview(&mut self) {
        self.quality_preview_rows.clear();
        self.quality_preview_error = None;
        self.quality_preview_worker = None;

        if self.input_dir.trim().is_empty() {
            return;
        }
        let input = PathBuf::from(&self.input_dir);
        if !input.is_dir() {
            return;
        }

        let sample = std::fs::read_dir(&input)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .and_then(ImageFormat::from_extension)
                        .is_some()
            });

        let Some(sample) = sample else {
            self.quality_preview_error = Some("输入文件夹中未找到可预览的图片".into());
            return;
        };

        self.quality_preview_worker = Some(QualityPreviewWorker::spawn(
            sample,
            self.formats[self.format_index],
        ));
    }

    pub(super) fn poll_quality_preview(&mut self) {
        let Some(worker) = &self.quality_preview_worker else {
            return;
        };
        if let Some(msg) = worker.poll() {
            self.quality_preview_worker = None;
            match msg {
                quality_preview::QualityPreviewMsg::Done(rows) => {
                    self.quality_preview_rows = rows;
                }
                quality_preview::QualityPreviewMsg::Failed(e) => {
                    self.quality_preview_error = Some(e);
                }
            }
        }
    }

    pub(super) fn record_history(&mut self, report: &ProcessReport) {
        let entry = TaskHistoryEntry {
            finished_at_unix: prefs::now_unix(),
            input_dir: self.input_dir.clone(),
            output_dir: self.output_dir.clone(),
            successes: report.successes,
            failures: report.failures.len(),
            total: report.total,
            elapsed_ms: report.elapsed.as_millis() as u64,
            snapshot: self.snapshot_from_ui(),
        };
        self.gui_prefs.push_history(entry);
        let _ = self.gui_prefs.save();
    }

    pub(super) fn start_conversion(&mut self) {
        if self.is_running() {
            return;
        }

        self.refresh_previews();

        let config = match self.build_config() {
            Ok(c) => c,
            Err(e) => {
                self.status = e.clone();
                self.push_log(format!("错误：{e}"));
                return;
            }
        };
        self.persist_brightness_match_prefs();

        if let Some(ref preview) = self.batch_preview {
            if preview.output_conflicts > 0 {
                self.status = format!(
                    "存在 {} 处输出路径冲突，请调整重命名模板",
                    preview.output_conflicts
                );
                self.push_log(self.status.clone());
                return;
            }
            if preview.to_convert == 0 {
                self.status = "没有需要转换的文件（可能均已存在且未勾选覆盖）".into();
                self.push_log(self.status.clone());
                return;
            }
        }

        if self.prefer_remote_execution {
            self.start_remote_conversion(config);
            return;
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let progress: Arc<dyn ProgressReporter> = Arc::new(GuiProgress::new());
        let (tx, rx) = mpsc::channel();

        self.worker_rx = Some(rx);
        self.state = RunState::Running {
            cancelled: Arc::clone(&cancelled),
            progress: Arc::clone(&progress),
        };
        self.status = "正在扫描并转换图片…".to_string();
        self.push_log(format!(
            "开始：{} → {} ({})",
            config.input_dir.display(),
            config.output_dir.display(),
            config.target_format
        ));

        thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(WorkerMessage::Finished(Err(e.to_string())));
                    return;
                }
            };

            let progress_reporter = Arc::clone(&progress);
            let result = rt.block_on(run_batch(config, cancelled, Some(progress_reporter)));
            let msg = match result {
                Ok(report) => WorkerMessage::Finished(Ok(report)),
                Err(e) => WorkerMessage::Finished(Err(e.to_string())),
            };
            let _ = tx.send(msg);
        });
    }

    fn start_remote_conversion(&mut self, config: crate::config::AppConfig) {
        if !config.remote.enabled || !config.remote.is_configured() {
            self.status =
                "已勾选优先远端，但远端未启用或未配置 base_url（见 [remote] / 环境变量）".into();
            self.push_log(self.status.clone());
            return;
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let progress: Arc<dyn ProgressReporter> = Arc::new(GuiProgress::new());
        let (tx, rx) = mpsc::channel();
        self.worker_rx = Some(rx);
        self.state = RunState::Running {
            cancelled: Arc::clone(&cancelled),
            progress,
        };
        self.status = "正在上传并远端转换…".into();
        self.push_log(format!(
            "远端转换：{} → {} ({}) @ {}",
            config.input_dir.display(),
            config.output_dir.display(),
            config.target_format,
            config.remote.base_url.as_deref().unwrap_or("(none)")
        ));

        let remote_cfg = config.remote.clone();
        thread::spawn(move || {
            if cancelled.load(Ordering::Relaxed) {
                let _ = tx.send(WorkerMessage::RemoteSubmitted {
                    job_id: None,
                    message: "已取消".into(),
                    ok: false,
                });
                return;
            }
            let client = match crate::remote::try_build_http_client(&remote_cfg) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerMessage::RemoteSubmitted {
                        job_id: None,
                        message: e.to_string(),
                        ok: false,
                    });
                    return;
                }
            };
            let sync = crate::remote::TaskSyncService::new(remote_cfg, client);
            match sync.run_convert_and_download(&config, Some(&cancelled)) {
                Ok(outcome) => {
                    let msg = format!(
                        "远端完成 {}：成功 {} / 失败 {}，已下载 {} 个文件",
                        outcome.status.job_id,
                        outcome.result.successes,
                        outcome.result.failures,
                        outcome.downloaded.len()
                    );
                    let ok = outcome.status.phase
                        == crate::remote::types::RemoteJobPhase::Succeeded
                        || outcome.result.successes > 0;
                    let _ = tx.send(WorkerMessage::RemoteSubmitted {
                        job_id: Some(outcome.status.job_id),
                        message: msg,
                        ok,
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::RemoteSubmitted {
                        job_id: None,
                        message: e.to_string(),
                        ok: false,
                    });
                }
            }
        });
    }

    pub(super) fn cancel_conversion(&mut self) {
        if let RunState::Running { cancelled, .. } = &self.state {
            cancelled.store(true, Ordering::Relaxed);
            self.status = "正在取消…".to_string();
            self.push_log("用户请求取消");
        }
    }

    pub(super) fn poll_worker(&mut self) {
        let Some(rx) = self.worker_rx.as_ref() else {
            return;
        };

        let Ok(msg) = rx.try_recv() else {
            return;
        };

        self.worker_rx = None;
        match msg {
            WorkerMessage::Finished(Ok(report)) => {
                self.last_failed_inputs = report.failures.iter().map(|f| f.path.clone()).collect();
                let summary = format!(
                    "完成：成功 {} / {}，失败 {}，耗时 {}",
                    report.successes,
                    report.total,
                    report.failures.len(),
                    humantime::format_duration(report.elapsed)
                );
                if report.cancelled {
                    self.status = format!("已取消（{summary}）");
                } else {
                    self.status = summary.clone();
                }
                self.push_log(summary);
                for failure in &report.failures {
                    self.push_log(format!(
                        "失败：{} — {}",
                        failure.path.display(),
                        failure.error
                    ));
                }
                self.record_history(&report);
                self.state = RunState::Done(report);
            }
            WorkerMessage::Finished(Err(e)) => {
                self.status = format!("转换失败：{e}");
                self.push_log(format!("错误：{e}"));
                self.state = RunState::Failed;
            }
            WorkerMessage::ImportFinished(Ok(result)) => {
                self.input_dir = result.staging_dir.display().to_string();
                let summary = format!(
                    "设备导入完成：共 {} 个文件（图片 {} · 视频 {}）→ {}",
                    result.file_count,
                    result.image_count,
                    result.video_count,
                    self.input_dir
                );
                self.status = summary.clone();
                self.push_log(summary);
                if result.video_count > 0 {
                    self.push_log(
                        "提示：视频可切换到「视频评审」并从该暂存目录导入；图片可直接开始转换或送审",
                    );
                }
                self.refresh_previews();
                self.state = RunState::Idle;
            }
            WorkerMessage::ImportFinished(Err(e)) => {
                self.status = format!("设备导入失败：{e}");
                self.push_log(format!("错误：{e}"));
                self.state = RunState::Failed;
            }
            WorkerMessage::RemoteSubmitted {
                job_id,
                message,
                ok,
            } => {
                self.push_log(message.clone());
                self.status = message;
                if ok {
                    self.sync_remote_jobs();
                    if let Some(id) = job_id {
                        self.push_log(format!("远端任务完成：{id}"));
                    }
                    self.state = RunState::Idle;
                } else {
                    self.state = RunState::Failed;
                }
            }
        }
    }

    pub(super) fn retry_convert_failures(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() || self.is_running() {
            return;
        }
        self.review_queue = paths;
        self.review_queue_params.clear();
        self.mode = AppMode::Convert;
        self.status = format!("已载入 {} 个失败项，可开始重试", self.review_queue.len());
        self.push_log(self.status.clone());
        self.refresh_previews();
    }

    pub(super) fn open_output_folder(&self) {
        if self.output_dir.trim().is_empty() {
            return;
        }
        let path = PathBuf::from(&self.output_dir);
        if path.exists() {
            let _ = open::that(&path);
        }
    }

    pub(super) fn settings_checkboxes(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let mut file_options = vec![
            (&mut self.recursive, "包含子文件夹"),
            (&mut self.preserve_structure, "保留目录结构"),
            (&mut self.overwrite, "覆盖已有文件"),
            (&mut self.strip_metadata, "移除 EXIF 元数据"),
        ];

        widgets::settings_subheading(ui, "文件选项");
        ui.add_space(theme::SETTINGS_HEADING_GAP);
        widgets::checkbox_grid(ui, &mut file_options, enabled, 2);

        ui.add_space(8.0);
        widgets::settings_subheading(ui, "高级");
        ui.add_space(theme::SETTINGS_HEADING_GAP);

        let mut labels: Vec<&str> = Vec::new();
        #[cfg(feature = "bayer")]
        labels.push("RAW");
        labels.push("亮度匹配");
        labels.push("远端");
        labels.push("JIRA");
        widgets::settings_section_tabs(ui, &mut self.convert_advanced_tab, &labels);
        ui.add_space(8.0);

        let tab = self.convert_advanced_tab.min(labels.len().saturating_sub(1));
        let mut i = 0usize;
        #[cfg(feature = "bayer")]
        {
            if tab == i {
                let mut raw_options =
                    [(&mut self.bayer_only, "仅解 Bayer/RAW（跳过缩放/锐化/水印）")];
                widgets::checkbox_grid(ui, &mut raw_options, enabled, 1);
                ui.label(
                    egui::RichText::new("非 RAW 全流水线同样不做缩放与锐化，以保留清晰度")
                        .size(11.0)
                        .weak(),
                );
            }
            i += 1;
        }
        if tab == i {
            ui.add_enabled_ui(enabled && !self.bayer_only, |ui| {
                let bm_was = self.brightness_match_enabled;
                ui.checkbox(
                    &mut self.brightness_match_enabled,
                    "亮度匹配：RAW 贴近同名 JPG",
                );
                if bm_was != self.brightness_match_enabled {
                    self.persist_brightness_match_prefs();
                }
                ui.label(
                    egui::RichText::new(
                        "相机匹配（矩阵+曲线+LUT）· Bayer 仅解除外 · 失败回退增益",
                    )
                    .size(11.0)
                    .weak(),
                );
                widgets::settings_row_gap(ui);

                widgets::settings_labeled_row(ui, "非 RAW", |ui| {
                    if widgets::toggle_chip(
                        ui,
                        "全局参考",
                        self.brightness_match_mode == BrightnessMatchMode::Global,
                        true,
                    ) {
                        self.brightness_match_mode = BrightnessMatchMode::Global;
                        self.persist_brightness_match_prefs();
                    }
                    if widgets::toggle_chip(
                        ui,
                        "按文件配对",
                        self.brightness_match_mode == BrightnessMatchMode::Paired,
                        true,
                    ) {
                        self.brightness_match_mode = BrightnessMatchMode::Paired;
                        self.persist_brightness_match_prefs();
                    }
                });
                ui.label(
                    egui::RichText::new(match self.brightness_match_mode {
                        BrightnessMatchMode::Global => {
                            "非 RAW 用下方全局参考；RAW 始终对同目录同名 jpg"
                        }
                        BrightnessMatchMode::Paired => {
                            "RAW / 非 RAW 均对同目录同名 jpg/jpeg/png/webp；无配对则跳过"
                        }
                    })
                    .size(11.0)
                    .weak(),
                );
                widgets::settings_row_gap(ui);

                if self.brightness_match_mode == BrightnessMatchMode::Global {
                    self.ensure_brightness_match_preview(ui.ctx());
                    let path_issue = self.brightness_match_path_issue();
                    let drop_zone = ui.group(|ui| {
                        widgets::settings_labeled_row(ui, "参考图", |ui| {
                            if let Some((_, tex)) = &self.brightness_match_preview {
                                ui.add(
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(48.0, 48.0)),
                                );
                            }
                            if widgets::compact_secondary_button(ui, "选择…", true).clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
                                    .pick_file()
                                {
                                    self.set_brightness_match_path(path);
                                }
                            }
                            if widgets::compact_secondary_button(
                                ui,
                                "从输入目录",
                                !self.input_dir.trim().is_empty(),
                            )
                            .clicked()
                            {
                                self.pick_reference_from_input_dir();
                            }
                            if widgets::compact_secondary_button(
                                ui,
                                "清除",
                                !self.brightness_match_path.is_empty(),
                            )
                            .clicked()
                            {
                                self.clear_brightness_match_path();
                            }
                            let (display, full) = if self.brightness_match_path.is_empty() {
                                ("未选择参考图".to_string(), String::new())
                            } else {
                                let p = PathBuf::from(&self.brightness_match_path);
                                let name = p
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| self.brightness_match_path.clone());
                                (name, self.brightness_match_path.clone())
                            };
                            let resp = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(display).size(12.0).weak(),
                                )
                                .truncate(),
                            );
                            if !full.is_empty() {
                                resp.on_hover_text(full);
                            }
                        });
                        if path_issue.is_none()
                            && self.brightness_match_enabled
                            && !self.brightness_match_path.is_empty()
                            && self.brightness_match_preview.is_none()
                        {
                            ui.label(
                                egui::RichText::new("无法预览参考图")
                                    .size(11.0)
                                    .color(theme::secondary_label(ui.visuals().dark_mode)),
                            );
                        }
                    });
                    if drop_zone.response.contains_pointer() {
                        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
                        for file in dropped {
                            if let Some(path) = file.path {
                                let ext = path
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("");
                                if path.is_file()
                                    && crate::io::reference_pick::is_reference_image_ext(ext)
                                {
                                    self.set_brightness_match_path(path);
                                    self.status = "已拖入参考图".into();
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(issue) = &path_issue {
                        ui.label(
                            egui::RichText::new(issue)
                                .size(11.0)
                                .color(theme::error_color(ui.visuals().dark_mode)),
                        );
                    }
                }

                widgets::settings_labeled_row(ui, "统计", |ui| {
                    if widgets::toggle_chip(
                        ui,
                        "均值",
                        !self.brightness_match_metric_percentile,
                        true,
                    ) {
                        self.brightness_match_metric_percentile = false;
                        self.persist_brightness_match_prefs();
                    }
                    if widgets::toggle_chip(
                        ui,
                        "百分位",
                        self.brightness_match_metric_percentile,
                        true,
                    ) {
                        self.brightness_match_metric_percentile = true;
                        self.persist_brightness_match_prefs();
                    }
                    if widgets::toggle_chip(ui, "分区", self.brightness_match_regional, true) {
                        self.brightness_match_regional = !self.brightness_match_regional;
                        self.persist_brightness_match_prefs();
                    }
                });
                if self.brightness_match_metric_percentile {
                    widgets::settings_labeled_row(ui, "百分位", |ui| {
                        let slider_w = ui.available_width().max(80.0);
                        let resp = ui.add_sized(
                            egui::vec2(slider_w, widgets::TOOLBAR_ROW_HEIGHT),
                            egui::Slider::new(&mut self.brightness_match_percentile, 90.0..=99.0)
                                .suffix("%"),
                        );
                        if resp.changed() {
                            self.persist_brightness_match_prefs();
                        }
                    });
                }
            });
            if self.bayer_only {
                ui.label(
                    egui::RichText::new("仅解 Bayer 模式下跳过亮度匹配")
                        .size(11.0)
                        .weak(),
                );
            }
        }
        i += 1;
        if tab == i {
            ui.add_enabled_ui(enabled, |ui| {
                ui.checkbox(
                    &mut self.prefer_remote_execution,
                    "优先远程执行（需 [remote]；失败不回退本地）",
                );
            });
            ui.label(
                egui::RichText::new(format!(
                    "远端：{}{}",
                    self.remote_config.status_label(),
                    self.remote_config
                        .base_url
                        .as_ref()
                        .map(|u| format!(" · {u}"))
                        .unwrap_or_default()
                ))
                .size(11.0)
                .weak(),
            );
        }
        i += 1;
        if tab == i {
            self.jira_settings_body(ui, enabled);
        }
    }

    fn jira_settings_body(&mut self, ui: &mut egui::Ui, enabled: bool) {
        ui.checkbox(&mut self.jira_config.enabled, "启用 JIRA 批量提 Bug");

        let jira_on = self.jira_config.enabled;
        ui.add_enabled_ui(jira_on && enabled, |ui| {
            let dark = ui.visuals().dark_mode;
            let row_h = widgets::TOOLBAR_ROW_HEIGHT;

            widgets::settings_row_gap(ui);
            ui.label(
                egui::RichText::new("连接")
                    .size(11.0)
                    .strong()
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(2.0);
            widgets::settings_span_row(ui, "Base URL", |ui, span_w| {
                let mut url = self.jira_config.base_url.clone().unwrap_or_default();
                let resp = ui.add_sized(
                    egui::vec2(span_w, row_h),
                    egui::TextEdit::singleline(&mut url)
                        .hint_text("https://your.atlassian.net")
                        .horizontal_align(egui::Align::Center),
                );
                if resp.changed() {
                    self.jira_config.base_url = if url.trim().is_empty() {
                        None
                    } else {
                        Some(url.trim().to_string())
                    };
                }
            });
            widgets::settings_pair_row(
                ui,
                "项目",
                "类型",
                |ui, field_w| {
                    let mut key = self.jira_config.project_key.clone().unwrap_or_default();
                    let resp = ui.add_sized(
                        egui::vec2(field_w, row_h),
                        egui::TextEdit::singleline(&mut key)
                            .hint_text("CAM")
                            .horizontal_align(egui::Align::Center),
                    );
                    if resp.changed() {
                        self.jira_config.project_key = if key.trim().is_empty() {
                            None
                        } else {
                            Some(key.trim().to_string())
                        };
                    }
                },
                |ui, field_w| {
                    ui.add_sized(
                        egui::vec2(field_w, row_h),
                        egui::TextEdit::singleline(&mut self.jira_config.issue_type)
                            .hint_text("Bug")
                            .horizontal_align(egui::Align::Center),
                    );
                },
            );
            widgets::settings_pair_row(
                ui,
                "API",
                "认证",
                |ui, field_w| {
                    let api_label = match self.jira_config.api_version {
                        crate::jira::JiraApiVersion::V3 => "v3 · Cloud",
                        crate::jira::JiraApiVersion::V2 => "v2 · Server",
                    };
                    widgets::toolbar_combo_box(ui, "jira_api_version", api_label, field_w, |ui| {
                        if ui
                            .selectable_label(
                                self.jira_config.api_version == crate::jira::JiraApiVersion::V3,
                                "v3 · Cloud",
                            )
                            .clicked()
                        {
                            self.jira_config.api_version = crate::jira::JiraApiVersion::V3;
                        }
                        if ui
                            .selectable_label(
                                self.jira_config.api_version == crate::jira::JiraApiVersion::V2,
                                "v2 · Server / DC",
                            )
                            .clicked()
                        {
                            self.jira_config.api_version = crate::jira::JiraApiVersion::V2;
                        }
                    });
                },
                |ui, field_w| {
                    let auth_label = match self.jira_config.auth_mode {
                        crate::jira::JiraAuthMode::EnvBasic => "Basic · Token",
                        crate::jira::JiraAuthMode::EnvBearer => "Bearer · PAT",
                    };
                    widgets::toolbar_combo_box(ui, "jira_auth_mode", auth_label, field_w, |ui| {
                        if ui
                            .selectable_label(
                                self.jira_config.auth_mode == crate::jira::JiraAuthMode::EnvBasic,
                                "Basic · 邮箱+Token",
                            )
                            .clicked()
                        {
                            self.jira_config.auth_mode = crate::jira::JiraAuthMode::EnvBasic;
                        }
                        if ui
                            .selectable_label(
                                self.jira_config.auth_mode == crate::jira::JiraAuthMode::EnvBearer,
                                "Bearer · PAT",
                            )
                            .clicked()
                        {
                            self.jira_config.auth_mode = crate::jira::JiraAuthMode::EnvBearer;
                        }
                    });
                },
            );
            widgets::settings_hint(
                ui,
                match self.jira_config.auth_mode {
                    crate::jira::JiraAuthMode::EnvBasic => {
                        "凭证：IMGFORGE_JIRA_EMAIL + IMGFORGE_JIRA_API_TOKEN（不落盘）"
                    }
                    crate::jira::JiraAuthMode::EnvBearer => "凭证：IMGFORGE_JIRA_PAT（不落盘）",
                },
            );

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("提交")
                    .size(11.0)
                    .strong()
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(2.0);
            widgets::settings_pair_row(
                ui,
                "附件",
                "并发",
                |ui, field_w| {
                    let attach_label = match (
                        self.jira_config.attach_screenshots,
                        self.jira_config.attach_defect_zip,
                    ) {
                        (true, true) => "截图 + zip",
                        (true, false) => "仅截图",
                        (false, true) => "仅 zip",
                        (false, false) => "不上传",
                    };
                    widgets::toolbar_combo_box(
                        ui,
                        "jira_attach_defaults",
                        attach_label,
                        field_w,
                        |ui| {
                            let cur = (
                                self.jira_config.attach_screenshots,
                                self.jira_config.attach_defect_zip,
                            );
                            let options = [
                                ((true, true), "截图 + 缺陷 zip"),
                                ((true, false), "仅截图"),
                                ((false, true), "仅缺陷 zip"),
                                ((false, false), "不上传"),
                            ];
                            for (val, label) in options {
                                if ui.selectable_label(cur == val, label).clicked() {
                                    self.jira_config.attach_screenshots = val.0;
                                    self.jira_config.attach_defect_zip = val.1;
                                }
                            }
                        },
                    );
                },
                |ui, field_w| {
                    let mut n = self.jira_config.max_concurrent.clamp(1, 4);
                    if ui
                        .add_sized(
                            egui::vec2(field_w, row_h),
                            egui::DragValue::new(&mut n).range(1..=4).speed(0.2),
                        )
                        .changed()
                    {
                        self.jira_config.max_concurrent = n;
                    }
                },
            );
            widgets::settings_hint(ui, "同时建单 1–4，1=串行");

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("字段映射")
                    .size(11.0)
                    .strong()
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(2.0);
            widgets::settings_pair_row(
                ui,
                "标签",
                "优先级",
                |ui, field_w| {
                    let mut labels = self.jira_config.labels.join(", ");
                    let resp = ui.add_sized(
                        egui::vec2(field_w, row_h),
                        egui::TextEdit::singleline(&mut labels)
                            .hint_text("imgforge, review")
                            .horizontal_align(egui::Align::Center),
                    );
                    if resp.changed() {
                        self.jira_config.labels = labels
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                },
                |ui, field_w| {
                    const OPTIONS: &[&str] = &["Highest", "High", "Medium", "Low", "Lowest"];
                    let current = if self.jira_config.default_priority.trim().is_empty() {
                        "Medium"
                    } else {
                        self.jira_config.default_priority.as_str()
                    };
                    let mut chosen = current.to_string();
                    widgets::toolbar_combo_box(
                        ui,
                        "jira_default_priority",
                        current,
                        field_w,
                        |ui| {
                            for opt in OPTIONS {
                                if ui.selectable_label(current == *opt, *opt).clicked() {
                                    chosen = (*opt).to_string();
                                }
                            }
                            if !OPTIONS.contains(&current) {
                                if ui
                                    .selectable_label(true, format!("{current}（自定义）"))
                                    .clicked()
                                {
                                    chosen = current.to_string();
                                }
                            }
                        },
                    );
                    if chosen != current {
                        self.jira_config.default_priority = chosen;
                    }
                },
            );
            widgets::settings_hint(ui, "视频缺陷仍按 S1→Highest … S5→Lowest 映射（内置）");
        });

        ui.add_space(6.0);
        widgets::settings_indented(ui, |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "状态：{} · 凭证 {}",
                    self.jira_config.status_label(),
                    if self.jira_config.has_credentials() {
                        "已检测到"
                    } else {
                        "未设置"
                    }
                ))
                .size(11.0)
                .weak(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if widgets::secondary_button(ui, "探活 myself", jira_on && enabled).clicked() {
                    self.jira_config.apply_env_overrides();
                    match crate::jira::JiraClient::probe(&self.jira_config) {
                        Ok(me) => {
                            let msg = format!("JIRA 探活成功：{}", me.display_name);
                            self.jira_probe_status = Some(msg.clone());
                            self.status = msg.clone();
                            self.push_log(msg);
                        }
                        Err(e) => {
                            let msg = format!("JIRA 探活失败：{e}");
                            self.jira_probe_status = Some(msg.clone());
                            self.status = msg.clone();
                            self.push_log(msg);
                        }
                    }
                }
                if widgets::primary_button(ui, "保存设置", true).clicked() {
                    self.persist_jira_prefs();
                    self.status = "已保存 JIRA 设置（不含凭证）".into();
                    self.push_log(self.status.clone());
                }
            });
            if let Some(status) = &self.jira_probe_status {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(status).size(11.0));
            }
        });
    }

    pub(super) fn persist_jira_prefs(&mut self) {
        self.gui_prefs.jira = crate::jira::JiraPrefsSnapshot::from_config(&self.jira_config);
        let _ = self.gui_prefs.save();
    }

    pub(super) fn sync_remote_jobs(&mut self) {
        let client = crate::remote::build_client(&self.remote_config);
        let sync = crate::remote::TaskSyncService::new(self.remote_config.clone(), client);
        match sync.sync_jobs(50) {
            Ok(snap) => {
                let msg = format!(
                    "远端同步完成：{} 个任务（{}）",
                    snap.jobs.len(),
                    if snap.from_cache { "缓存" } else { "实时" }
                );
                self.status = msg.clone();
                self.push_log(msg);
                self.remote_snapshot = Some(snap);
            }
            Err(e) => {
                let cached = sync.load_cached_snapshot();
                self.remote_snapshot = Some(cached);
                self.status = format!("远端同步失败：{e}");
                self.push_log(self.status.clone());
            }
        }
    }

    pub(super) fn refresh_remote_job(&mut self, job_id: &str) {
        let client = crate::remote::build_client(&self.remote_config);
        let sync = crate::remote::TaskSyncService::new(self.remote_config.clone(), client);
        match sync.refresh_job(job_id) {
            Ok(status) => {
                self.status = format!("远端任务 {} → {}", status.job_id, status.phase.label());
                self.push_log(self.status.clone());
                if let Ok(snap) = sync.sync_jobs(50) {
                    self.remote_snapshot = Some(snap);
                } else {
                    self.remote_snapshot = Some(sync.load_cached_snapshot());
                }
            }
            Err(e) => {
                self.status = format!("刷新远端任务失败：{e}");
                self.push_log(self.status.clone());
            }
        }
    }

    /// 转换页布局：文件夹同行对称；设置 |（预设+摘要+状态）等高双栏；日志全宽。
    pub(super) fn convert_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        dark: bool,
        enabled: bool,
        running: bool,
        log_height: f32,
        bottom_reserve: f32,
    ) {
        widgets::navigation_header(ui, "选择目录、设定格式，开始批量转换");
        widgets::page_header_gap(ui);

        if !self.review_queue.is_empty() {
            self.convert_review_queue_ui(ui, dark, enabled);
            widgets::section_gap(ui);
        }

        let wide = ui.available_width() >= theme::CONVERT_WIDE_BREAKPOINT;
        let gap = theme::SECTION_GAP;
        // 与 SideMainGeometry 同一套思路：宽屏等分显式定宽，窄屏纵向堆叠，避免侧块被挤出。

        // 文件夹：同一卡片内左右等分，高度天然对齐
        widgets::grouped_section(ui, "文件夹", |ui| {
            if wide {
                let half = ((ui.available_width() - gap) * 0.5).max(200.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.set_width(ui.available_width());
                    ui.vertical(|ui| {
                        ui.set_width(half);
                        let prev = self.input_dir.clone();
                        widgets::folder_field(ui, "输入", &mut self.input_dir, enabled);
                        if prev != self.input_dir {
                            self.refresh_previews();
                        }
                        if self.input_dir.trim().is_empty() {
                            widgets::drop_hint(ui);
                        }
                    });
                    ui.add_space(gap);
                    ui.vertical(|ui| {
                        ui.set_width(half);
                        widgets::folder_field(ui, "输出", &mut self.output_dir, enabled);
                        ui.label(
                            eframe::egui::RichText::new("转换结果写入此目录")
                                .size(12.0)
                                .color(theme::secondary_label(dark)),
                        );
                    });
                });
            } else {
                let prev = self.input_dir.clone();
                widgets::folder_field(ui, "输入", &mut self.input_dir, enabled);
                if prev != self.input_dir {
                    self.refresh_previews();
                }
                if self.input_dir.trim().is_empty() {
                    widgets::drop_hint(ui);
                }
                widgets::folder_field(ui, "输出", &mut self.output_dir, enabled);
            }
        });
        widgets::section_gap(ui);

        self.convert_device_import_ui(ui, dark, enabled);
        widgets::section_gap(ui);

        if wide {
            let half = ((ui.available_width() - gap) * 0.5).max(280.0);
            // 不用 available_height 撑满视口，避免右侧大块留白
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.vertical(|ui| {
                    ui.set_width(half);
                    self.convert_settings_ui(ui, dark, enabled);
                });
                ui.add_space(gap);
                ui.vertical(|ui| {
                    ui.set_width(half);
                    self.convert_presets_ui(ui, dark, enabled);
                    widgets::section_gap(ui);
                    self.convert_preview_ui(ui, dark, enabled);
                    widgets::section_gap(ui);
                    widgets::grouped_section(ui, "运行状态", |ui| {
                        self.convert_run_status_ui(ui, dark, running);
                    });
                });
            });
        } else {
            self.convert_settings_ui(ui, dark, enabled);
            widgets::section_gap(ui);
            self.convert_presets_ui(ui, dark, enabled);
            widgets::section_gap(ui);
            self.convert_preview_ui(ui, dark, enabled);
            widgets::section_gap(ui);
            widgets::grouped_section(ui, "运行状态", |ui| {
                self.convert_run_status_ui(ui, dark, running);
            });
        }

        widgets::section_gap(ui);
        widgets::log_panel(ui, &self.log_lines, log_height);
        ui.add_space(bottom_reserve);
    }

    fn convert_device_import_ui(&mut self, ui: &mut egui::Ui, dark: bool, enabled: bool) {
        use eframe::egui::{Layout, RichText};
        use crate::mobile::MobilePullBackend;

        const ROW_GAP: f32 = 8.0;
        const FIELD_GAP: f32 = 6.0;
        let field_h = widgets::TOOLBAR_ROW_HEIGHT;

        widgets::grouped_section(ui, "设备导入", |ui| {
            ui.label(
                RichText::new("从手机、运动相机等导入图片/视频到本地暂存目录，再用于转换或评审")
                    .size(12.0)
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(8.0);

            let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
            // 设备导入区比全局窄断点更早进入紧凑，避免侧栏小窗横向挤压
            let compact = ui.available_width() < 440.0;

            let label_cell = |ui: &mut egui::Ui, text: &str| {
                ui.allocate_ui_with_layout(
                    egui::vec2(theme::SETTINGS_LABEL_WIDTH, field_h),
                    Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new(text)
                                .font(theme::section_font())
                                .color(theme::primary_label(dark)),
                        );
                    },
                );
            };

            // 控件列吃满标签后的全部剩余宽度，与上方「文件夹」/下方右栏右缘对齐
            let control_row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                if narrow || compact {
                    if !label.is_empty() {
                        ui.label(
                            RichText::new(label)
                                .font(theme::section_font())
                                .color(theme::primary_label(dark)),
                        );
                        ui.add_space(4.0);
                    }
                    let w = ui.available_width().max(80.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(w, field_h),
                        Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_width(w);
                            add(ui);
                        },
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.x = 0.0;
                        label_cell(ui, label);
                        ui.add_space(FIELD_GAP);
                        let w = ui.available_width().max(80.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(w, field_h),
                            Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_width(w);
                                add(ui);
                            },
                        );
                    });
                }
            };

            // 方式：与来源/暂存/设备/并发共用 settings_span_row，左缘对齐
            if compact || narrow {
                ui.label(
                    RichText::new("方式")
                        .font(theme::section_font())
                        .color(theme::primary_label(dark)),
                );
                ui.add_space(4.0);
                let w = ui.available_width().max(80.0);
                let label = match self.mobile_backend {
                    MobilePullBackend::Auto => "自动（挂载优先，否则 ADB）",
                    MobilePullBackend::Fs => "本地挂载（U 盘 / SD 卡）",
                    MobilePullBackend::Adb => "ADB（移动设备）",
                };
                widgets::toolbar_combo_box(ui, "mobile_backend", label, w, |ui| {
                    for (backend, text) in [
                        (
                            MobilePullBackend::Auto,
                            "自动（挂载优先，否则 ADB）",
                        ),
                        (MobilePullBackend::Fs, "本地挂载（U 盘 / SD 卡）"),
                        (MobilePullBackend::Adb, "ADB（移动设备）"),
                    ] {
                        if ui
                            .selectable_label(self.mobile_backend == backend, text)
                            .clicked()
                        {
                            self.mobile_backend = backend;
                            if matches!(backend, MobilePullBackend::Adb)
                                && !self.mobile_source.starts_with('/')
                            {
                                self.mobile_source = "/sdcard/DCIM".into();
                            }
                        }
                    }
                });
            } else {
                widgets::settings_span_row(ui, "方式", |ui, span_w| {
                    let label = match self.mobile_backend {
                        MobilePullBackend::Auto => "自动（挂载优先，否则 ADB）",
                        MobilePullBackend::Fs => "本地挂载（U 盘 / SD 卡）",
                        MobilePullBackend::Adb => "ADB（移动设备）",
                    };
                    widgets::toolbar_combo_box(ui, "mobile_backend", label, span_w, |ui| {
                        for (backend, text) in [
                            (
                                MobilePullBackend::Auto,
                                "自动（挂载优先，否则 ADB）",
                            ),
                            (MobilePullBackend::Fs, "本地挂载（U 盘 / SD 卡）"),
                            (MobilePullBackend::Adb, "ADB（移动设备）"),
                        ] {
                            if ui
                                .selectable_label(self.mobile_backend == backend, text)
                                .clicked()
                            {
                                self.mobile_backend = backend;
                                if matches!(backend, MobilePullBackend::Adb)
                                    && !self.mobile_source.starts_with('/')
                                {
                                    self.mobile_source = "/sdcard/DCIM".into();
                                }
                            }
                        }
                    });
                });
            }
            ui.add_space(ROW_GAP);

            // 来源 / 暂存 / 设备：settings_path_action_row 内部已处理小窗堆叠
            let source_hint = match self.mobile_backend {
                MobilePullBackend::Fs => "选择设备挂载目录…",
                MobilePullBackend::Adb => "默认路径（刷新设备后可按台修改）",
                MobilePullBackend::Auto => "/sdcard/DCIM 或本地挂载路径",
            };
            let source_needs_browse = matches!(
                self.mobile_backend,
                MobilePullBackend::Fs | MobilePullBackend::Auto
            );
            let align_trailing = !matches!(self.mobile_backend, MobilePullBackend::Fs);
            let browse_label = if compact { "浏览" } else { "浏览…" };
            let refresh_label = if compact { "刷新" } else { "刷新设备" };

            let source_browse = widgets::settings_path_action_row(
                ui,
                "来源",
                &mut self.mobile_source,
                source_hint,
                enabled,
                source_needs_browse.then_some(browse_label),
                align_trailing && !source_needs_browse,
            );
            if source_browse {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.mobile_source = folder.display().to_string();
                    if matches!(self.mobile_backend, MobilePullBackend::Auto) {
                        self.mobile_backend = MobilePullBackend::Fs;
                    }
                }
            }
            ui.add_space(ROW_GAP);

            let staging_browse = widgets::settings_path_action_row(
                ui,
                "暂存",
                &mut self.mobile_staging,
                "共用保存根目录；未单独指定时按设备建子文件夹",
                enabled,
                Some(browse_label),
                false,
            );
            if staging_browse {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.mobile_staging = folder.display().to_string();
                }
            }
            ui.add_space(ROW_GAP);

            // 设备
            if !matches!(self.mobile_backend, MobilePullBackend::Fs) {
                let refresh = widgets::settings_path_action_row(
                    ui,
                    "设备",
                    &mut self.mobile_adb_serial,
                    "或手动填写 serial，可逗号分隔",
                    enabled,
                    Some(refresh_label),
                    false,
                );
                if refresh {
                    self.refresh_adb_devices();
                }
                if self.mobile_adb_devices_loaded {
                    ui.add_space(4.0);
                    let n = self.mobile_adb_devices.len();
                    let selected = self
                        .mobile_adb_devices
                        .iter()
                        .filter(|row| row.selected)
                        .count();
                    ui.label(
                        egui::RichText::new(format!(
                            "已识别 {n} 台，已勾选 {selected}（勾选后点导入）"
                        ))
                        .size(11.0)
                        .weak(),
                    );
                    if n > 0 {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(enabled, egui::Button::new("全选"))
                                .clicked()
                            {
                                for row in &mut self.mobile_adb_devices {
                                    row.selected = true;
                                }
                            }
                            if ui
                                .add_enabled(enabled, egui::Button::new("全不选"))
                                .clicked()
                            {
                                for row in &mut self.mobile_adb_devices {
                                    row.selected = false;
                                }
                            }
                        });
                    }
                }
                if !self.mobile_adb_devices.is_empty() {
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        for row in &mut self.mobile_adb_devices {
                            ui.add_enabled_ui(enabled, |ui| {
                                ui.checkbox(&mut row.selected, row.info.display_label());
                                let field_w = (ui.available_width() - 36.0).max(80.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(18.0);
                                    ui.label(egui::RichText::new("来源").size(11.0).weak());
                                    ui.add_sized(
                                        egui::vec2(field_w, field_h * 0.9),
                                        egui::TextEdit::singleline(&mut row.source_path)
                                            .hint_text("/sdcard/DCIM"),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.add_space(18.0);
                                    ui.label(egui::RichText::new("保存").size(11.0).weak());
                                    let staging_hint = if self.mobile_staging.trim().is_empty() {
                                        "留空则须填写上方「暂存」".to_string()
                                    } else {
                                        format!("留空 → {}/<serial>", self.mobile_staging.trim())
                                    };
                                    ui.add_sized(
                                        egui::vec2(field_w, field_h * 0.9),
                                        egui::TextEdit::singleline(&mut row.staging_path)
                                            .hint_text(staging_hint),
                                    );
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
                }
                ui.add_space(ROW_GAP);
            }

            // 并发：与「设备」等路径行共用 settings_span_row，保证控件左缘对齐
            let concurrency_hint = "每台设备内文件并发（1–8）";
            if compact || narrow {
                ui.label(
                    RichText::new("并发")
                        .font(theme::section_font())
                        .color(theme::primary_label(dark)),
                );
                ui.add_space(4.0);
                ui.add_sized(
                    egui::vec2(88.0_f32.min(ui.available_width()), field_h),
                    egui::DragValue::new(&mut self.mobile_pull_concurrency)
                        .range(1..=8)
                        .speed(0.2)
                        .suffix(" 路"),
                );
                ui.add_space(2.0);
                ui.label(egui::RichText::new(concurrency_hint).size(11.0).weak());
            } else {
                widgets::settings_span_row(ui, "并发", |ui, _span_w| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add_sized(
                            egui::vec2(88.0, field_h),
                            egui::DragValue::new(&mut self.mobile_pull_concurrency)
                                .range(1..=8)
                                .speed(0.2)
                                .suffix(" 路"),
                        );
                        ui.label(
                            egui::RichText::new(concurrency_hint)
                                .size(12.0)
                                .color(theme::secondary_label(dark)),
                        );
                    });
                });
            }
            ui.add_space(ROW_GAP);

            // 导入按钮：与路径行同一套 span 几何（不受 compact 影响）
            if widgets::settings_primary_action_row(ui, "从设备导入", enabled).clicked() {
                self.start_device_import();
            }
        });
    }

    pub(super) fn refresh_adb_devices(&mut self) {
        use crate::mobile::{list_ready_devices, MobilePullConfig};
        use super::MobileAdbDeviceRow;

        let config = MobilePullConfig {
            enabled: true,
            backend: crate::mobile::MobilePullBackend::Adb,
            ..MobilePullConfig::default()
        };
        match list_ready_devices(&config) {
            Ok(devices) => {
                let n = devices.len();
                let default_path = if self.mobile_source.trim().is_empty() {
                    "/sdcard/DCIM".to_string()
                } else {
                    self.mobile_source.trim().to_string()
                };
                // 保留同 serial 已填路径与勾选；新设备默认不勾选
                let previous = std::mem::take(&mut self.mobile_adb_devices);
                self.mobile_adb_devices = devices
                    .into_iter()
                    .map(|info| {
                        if let Some(prev) =
                            previous.iter().find(|row| row.info.serial == info.serial)
                        {
                            MobileAdbDeviceRow {
                                info,
                                selected: prev.selected,
                                source_path: if prev.source_path.trim().is_empty() {
                                    default_path.clone()
                                } else {
                                    prev.source_path.clone()
                                },
                                staging_path: prev.staging_path.clone(),
                            }
                        } else {
                            MobileAdbDeviceRow {
                                info,
                                selected: false,
                                source_path: default_path.clone(),
                                staging_path: String::new(),
                            }
                        }
                    })
                    .collect();
                self.mobile_adb_devices_loaded = true;
                if n == 0 {
                    self.status = "未发现已授权 ADB 设备".into();
                } else {
                    self.status = format!("已识别 {n} 台设备（请勾选、确认来源/保存路径后导入）");
                }
                self.push_log(self.status.clone());
            }
            Err(e) => {
                self.mobile_adb_devices.clear();
                self.mobile_adb_devices_loaded = false;
                self.status = format!("刷新设备失败：{e}");
                self.push_log(self.status.clone());
            }
        }
    }

    pub(super) fn start_device_import(&mut self) {
        use crate::gui::app_types::DeviceImportResult;
        use crate::mobile::{import_media, parse_serial_list, AdbDevicePull, MobilePullConfig};

        if self.is_running() {
            return;
        }
        if self.mobile_staging.trim().is_empty() {
            let any_without_staging = self.mobile_adb_devices.iter().any(|row| {
                row.selected && row.staging_path.trim().is_empty()
            });
            let manual_serials = parse_serial_list(&self.mobile_adb_serial);
            if any_without_staging || !manual_serials.is_empty() {
                // 手动 serial 依赖全局暂存；勾选但未填保存路径也依赖全局暂存
                self.status = "请填写共用「暂存」目录，或为每台勾选设备单独填写「保存」路径".into();
                self.push_log(self.status.clone());
                return;
            }
        }

        let default_source = self.mobile_source.trim().to_string();
        let mut adb_devices: Vec<AdbDevicePull> = self
            .mobile_adb_devices
            .iter()
            .filter(|row| row.selected)
            .map(|row| {
                let src = row.source_path.trim();
                let staging = row.staging_path.trim();
                AdbDevicePull::with_paths(
                    row.info.serial.clone(),
                    if src.is_empty() {
                        default_source.as_str()
                    } else {
                        src
                    },
                    if staging.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(staging))
                    },
                )
            })
            .collect();

        // 手动 serial：用全局来源路径 + 共用暂存/<serial>
        for s in parse_serial_list(&self.mobile_adb_serial) {
            if adb_devices.iter().any(|d| d.serial == s) {
                continue;
            }
            if default_source.is_empty() {
                self.status = format!("设备 {s} 需要来源路径：请填写上方「来源」或在列表中勾选并填路径");
                self.push_log(self.status.clone());
                return;
            }
            adb_devices.push(AdbDevicePull::new(s, default_source.clone()));
        }

        // GUI：必须明确勾选或填写，禁止「未指定 → 自动拉全部」
        if adb_devices.is_empty() {
            self.status = "请先刷新并勾选要导入的设备，或填写 serial".into();
            self.push_log(self.status.clone());
            return;
        }

        for device in &adb_devices {
            if device.resolved_source(&default_source).is_empty() {
                self.status = format!(
                    "设备 {} 的来源路径为空，请填写路径",
                    device.serial
                );
                self.push_log(self.status.clone());
                return;
            }
        }

        let adb_serials: Vec<String> = adb_devices.iter().map(|d| d.serial.clone()).collect();
        let adb_serial = if adb_serials.len() == 1 {
            Some(adb_serials[0].clone())
        } else {
            None
        };

        let source_path = if default_source.is_empty() {
            adb_devices
                .first()
                .and_then(|d| d.source_path.clone())
                .unwrap_or_else(|| "/sdcard/DCIM".into())
        } else {
            default_source
        };

        let staging_dir = if self.mobile_staging.trim().is_empty() {
            // 全部设备都有独立保存路径时，用第一台的保存目录作为 outcome 根
            adb_devices
                .iter()
                .find_map(|d| d.staging_override().cloned())
                .unwrap_or_else(|| PathBuf::from(".imgforge/mobile-import"))
        } else {
            PathBuf::from(self.mobile_staging.trim())
        };

        let config = MobilePullConfig {
            enabled: true,
            backend: self.mobile_backend,
            source_path,
            staging_dir,
            preserve_structure: true,
            adb_serial,
            adb_serials,
            adb_devices,
            adb_mode: crate::mobile::AdbBinaryMode::Auto,
            adb_path: None,
            allow_path_fallback: true,
            delete_after_pull: false,
            concurrency: self.mobile_pull_concurrency.clamp(1, 8),
        };

        if let Err(e) = config.validate() {
            self.status = e.to_string();
            self.push_log(format!("错误：{e}"));
            return;
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let progress: Arc<dyn ProgressReporter> = Arc::new(GuiProgress::new());
        let (tx, rx) = mpsc::channel();
        self.worker_rx = Some(rx);
        self.state = RunState::Running {
            cancelled: Arc::clone(&cancelled),
            progress: Arc::clone(&progress),
        };
        self.status = "正在从设备导入媒体…".into();
        let targets_summary: Vec<String> = config
            .adb_devices
            .iter()
            .map(|d| {
                format!(
                    "{}:{}",
                    d.serial,
                    d.resolved_source(&config.source_path)
                )
            })
            .collect();
        self.push_log(format!(
            "设备导入：{:?} [{}] → 共用暂存 {}",
            config.backend,
            targets_summary.join(", "),
            config.staging_dir.display()
        ));
        for d in &config.adb_devices {
            let root = crate::mobile::resolve_device_staging_root(
                &d.serial,
                d.staging_override(),
                &config.staging_dir,
            );
            self.push_log(format!(
                "  {} 来源={} 保存={}",
                d.serial,
                d.resolved_source(&config.source_path),
                root.display()
            ));
        }

        thread::spawn(move || {
            let result = import_media(config, cancelled, Some(progress));
            let msg = match result {
                Ok(outcome) => {
                    let mut image_count = 0usize;
                    let mut video_count = 0usize;
                    for path in &outcome.files {
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(str::to_ascii_lowercase)
                            .unwrap_or_default();
                        if matches!(
                            ext.as_str(),
                            "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "mts" | "m2ts" | "lrv"
                        ) {
                            video_count += 1;
                        } else {
                            image_count += 1;
                        }
                    }
                    WorkerMessage::ImportFinished(Ok(DeviceImportResult {
                        staging_dir: outcome.staging_dir,
                        file_count: outcome.files.len(),
                        image_count,
                        video_count,
                    }))
                }
                Err(e) => WorkerMessage::ImportFinished(Err(e.to_string())),
            };
            let _ = tx.send(msg);
        });
    }

    fn convert_review_queue_ui(&mut self, ui: &mut egui::Ui, dark: bool, enabled: bool) {
        use eframe::egui::RichText;
        use crate::gui::theme;

        widgets::grouped_section(ui, "评审队列", |ui| {
            ui.label(format!(
                "已从评审导入 {} 张「通过」图片，将仅转换这些文件",
                self.review_queue.len()
            ));
            ui.horizontal_wrapped(|ui| {
                for path in self.review_queue.iter().take(8) {
                    let label = if let Some(panel) = &self.review_panel {
                        panel
                            .status_for_path(path)
                            .map(|s| {
                                format!(
                                    "[{}] {}",
                                    s.label(),
                                    path.file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("")
                                )
                            })
                            .unwrap_or_else(|| {
                                path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("")
                                    .to_string()
                            })
                    } else {
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string()
                    };
                    ui.label(
                        RichText::new(label)
                            .size(12.0)
                            .color(theme::secondary_label(dark)),
                    );
                }
                if self.review_queue.len() > 8 {
                    ui.label(format!("…等 {} 张", self.review_queue.len()));
                }
            });
            ui.horizontal(|ui| {
                ui.add_enabled(
                    enabled,
                    egui::Checkbox::new(&mut self.burn_review_annotations, "导出时叠加标注"),
                );
                if widgets::secondary_button(ui, "清空评审队列", enabled).clicked() {
                    self.review_queue.clear();
                    self.review_queue_params.clear();
                    self.status = "已清空评审导入队列".into();
                }
                if widgets::compact_primary_button(
                    ui,
                    "发送到评审",
                    enabled && !self.review_queue.is_empty(),
                )
                .clicked()
                {
                    if let Some(panel) = &mut self.review_panel {
                        panel.schedule_import_from_queue(self.review_queue.clone(), "转换队列");
                        self.mode = AppMode::Review;
                        self.status = format!(
                            "已将 {} 张图片发送到评审模块",
                            self.review_queue.len()
                        );
                    }
                }
            });
        });
    }

    fn convert_settings_ui(&mut self, ui: &mut egui::Ui, dark: bool, enabled: bool) {
        use eframe::egui::RichText;
        use crate::gui::theme;

        widgets::grouped_section(ui, "转换设置", |ui| {
            widgets::settings_span_row(ui, "目标格式", |ui, span_w| {
                let label = self.formats[self.format_index]
                    .extension()
                    .to_uppercase();
                widgets::toolbar_combo_box(ui, "format", &label, span_w, |ui| {
                    for (idx, format) in self.formats.iter().enumerate() {
                        if ui
                            .selectable_label(
                                self.format_index == idx,
                                format.extension().to_uppercase(),
                            )
                            .clicked()
                        {
                            self.format_index = idx;
                        }
                    }
                });
            });

            widgets::settings_row_gap(ui);
            widgets::quality_slider_row(ui, &mut self.quality, enabled && !self.use_target_max_bytes);

            widgets::settings_row_gap(ui);
            // 左：勾选文案；右：体积数值 —— 控件列左右外缘对齐
            widgets::settings_span_row(ui, "目标体积", |ui, span_w| {
                let h = widgets::TOOLBAR_ROW_HEIGHT;
                let drag_w = 88.0_f32.min(span_w);
                let (span, _) = ui.allocate_exact_size(egui::vec2(span_w, h), egui::Sense::hover());
                let drag_rect = egui::Rect::from_min_size(
                    egui::pos2(span.max.x - drag_w, span.min.y),
                    egui::vec2(drag_w, h),
                );
                let check_rect = egui::Rect::from_min_max(
                    span.min,
                    egui::pos2((drag_rect.min.x - 6.0).max(span.min.x), span.max.y),
                );
                ui.allocate_ui_at_rect(check_rect, |ui| {
                    ui.set_min_size(check_rect.size());
                    ui.set_max_size(check_rect.size());
                    ui.with_layout(
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.checkbox(&mut self.use_target_max_bytes, "限制单文件 ≤");
                        },
                    );
                });
                ui.allocate_ui_at_rect(drag_rect, |ui| {
                    ui.set_min_size(drag_rect.size());
                    ui.set_max_size(drag_rect.size());
                    ui.add_enabled_ui(enabled && self.use_target_max_bytes, |ui| {
                        ui.add_sized(
                            drag_rect.size(),
                            egui::DragValue::new(&mut self.target_max_kb)
                                .range(16..=20_480)
                                .suffix(" KB"),
                        );
                    });
                });
            });
            if self.use_target_max_bytes {
                widgets::settings_hint(
                    ui,
                    "启用后将对 JPEG/WebP 等自动二分搜索质量以控制体积",
                );
            }

            widgets::settings_row_gap(ui);
            widgets::settings_span_row(ui, "重命名", |ui, span_w| {
                let response = ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        egui::vec2(span_w.max(80.0), widgets::TOOLBAR_ROW_HEIGHT),
                        egui::TextEdit::singleline(&mut self.rename_template)
                            .hint_text("{dir}_{stem}_{index}"),
                    )
                });
                if response.response.changed() {
                    self.refresh_previews();
                }
            });
            if let Some(err) = &self.rename_preview_error {
                ui.colored_label(theme::error_color(dark), err);
            } else if !self.rename_preview.is_empty() {
                widgets::settings_hint(ui, "预览输出名");
                for (src, out) in &self.rename_preview {
                    widgets::settings_indented(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{src} → {out}"))
                                .size(11.0)
                                .family(egui::FontFamily::Monospace),
                        );
                    });
                }
            }

            widgets::inset_separator(ui);
            self.settings_checkboxes(ui, enabled);
        });
    }

    fn convert_presets_ui(&mut self, ui: &mut egui::Ui, dark: bool, enabled: bool) {
        use eframe::egui::RichText;
        use crate::gui::theme;

        widgets::grouped_section(ui, "预设与历史", |ui| {
            let total_w = ui.available_width().max(120.0);
            let row_h = widgets::TOOLBAR_ROW_HEIGHT;
            // 加宽一点，避免中文换行把按钮撑高
            let btn_w = (widgets::toolbar_control_width(ui, "保存当前为预设") + 16.0)
                .clamp(128.0, (total_w * 0.55).max(128.0));
            let (row_rect, _) =
                ui.allocate_exact_size(egui::vec2(total_w, row_h), egui::Sense::hover());
            let btn_rect = egui::Rect::from_min_size(
                egui::pos2(row_rect.max.x - btn_w, row_rect.min.y),
                egui::vec2(btn_w, row_h),
            );
            let edit_rect = egui::Rect::from_min_max(
                row_rect.min,
                egui::pos2((btn_rect.min.x - 8.0).max(row_rect.min.x), row_rect.max.y),
            );
            ui.allocate_ui_at_rect(edit_rect, |ui| {
                ui.set_enabled(enabled);
                ui.add_sized(
                    edit_rect.size(),
                    egui::TextEdit::singleline(&mut self.new_preset_name)
                        .hint_text("预设名称")
                        .margin(egui::vec2(12.0, 6.0)),
                );
            });
            let save_clicked = ui
                .allocate_ui_at_rect(btn_rect, |ui| {
                    let accent = theme::accent(dark);
                    let btn = egui::Button::new(
                        RichText::new("保存当前为预设")
                            .size(13.0)
                            .strong()
                            .color(eframe::egui::Color32::WHITE),
                    )
                    .fill(if enabled {
                        accent
                    } else {
                        accent.linear_multiply(0.45)
                    })
                    .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS));
                    ui.add_enabled_ui(enabled, |ui| ui.add_sized(btn_rect.size(), btn))
                        .inner
                        .clicked()
                })
                .inner;
            if save_clicked {
                let name = self.new_preset_name.trim().to_string();
                if name.is_empty() {
                    self.status = "请输入预设名称".into();
                } else {
                    self.persist_brightness_match_prefs();
                    self.gui_prefs
                        .upsert_preset(name.clone(), self.snapshot_from_ui());
                    let _ = self.gui_prefs.save();
                    self.status = format!("已保存预设「{name}」");
                    self.new_preset_name.clear();
                }
            }

            if self.gui_prefs.presets.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("暂无自定义预设")
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                );
            } else {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    let labels: Vec<String> = self
                        .gui_prefs
                        .presets
                        .iter()
                        .map(|p| p.name.clone())
                        .collect();
                    let mut selected = self
                        .selected_preset
                        .unwrap_or(0)
                        .min(labels.len().saturating_sub(1));
                    egui::ComboBox::from_id_salt("user_preset")
                        .selected_text(
                            labels
                                .get(selected)
                                .cloned()
                                .unwrap_or_else(|| "选择预设".into()),
                        )
                        .show_ui(ui, |ui| {
                            for (i, name) in labels.iter().enumerate() {
                                ui.selectable_value(&mut selected, i, name);
                            }
                        });
                    self.selected_preset = Some(selected);
                    if widgets::compact_secondary_button(ui, "套用", enabled).clicked() {
                        if let Some(p) = self.gui_prefs.presets.get(selected).cloned() {
                            let name = p.name.clone();
                            self.apply_snapshot(&p.snapshot);
                            self.status = format!("已套用预设「{name}」");
                        }
                    }
                    if widgets::compact_secondary_button(ui, "删除", enabled).clicked() {
                        if let Some(name) = labels.get(selected).cloned() {
                            self.gui_prefs.delete_preset(&name);
                            let _ = self.gui_prefs.save();
                            self.selected_preset = None;
                        }
                    }
                });
            }

            if !self.gui_prefs.history.is_empty() {
                widgets::inset_separator(ui);
                widgets::settings_subheading(ui, "最近任务");
                ui.add_space(4.0);
                let recent: Vec<_> = self.gui_prefs.history.iter().take(5).cloned().collect();
                for (i, entry) in recent.into_iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} → {} · {}/{} 成功",
                                entry.input_dir,
                                entry.output_dir,
                                entry.successes,
                                entry.total
                            ))
                            .size(12.0)
                            .color(theme::secondary_label(dark)),
                        );
                        if widgets::compact_secondary_button(ui, "重跑", enabled).clicked() {
                            self.input_dir = entry.input_dir.clone();
                            self.output_dir = entry.output_dir.clone();
                            self.apply_snapshot(&entry.snapshot);
                            self.status = format!("已载入历史任务 #{i}");
                        }
                        if widgets::compact_secondary_button(ui, "打开输出", true).clicked() {
                            let path = PathBuf::from(&entry.output_dir);
                            if path.exists() {
                                let _ = open::that(&path);
                            }
                        }
                    });
                }
            }
        });
    }

    fn convert_preview_ui(&mut self, ui: &mut egui::Ui, dark: bool, enabled: bool) {
        use eframe::egui::RichText;
        use crate::gui::theme;

        widgets::grouped_section(ui, "转换前摘要", |ui| {
            widgets::equal_height_row(ui, 6.0, |ui| {
                if widgets::compact_secondary_button(ui, "刷新预估", enabled).clicked() {
                    self.refresh_previews();
                }
                if widgets::compact_secondary_button(ui, "质量体积预览", enabled).clicked() {
                    self.request_quality_preview();
                }
            });
            if let Some(ref preview) = self.batch_preview {
                for line in preview.summary_lines(self.formats[self.format_index].extension()) {
                    ui.label(RichText::new(line).size(12.0));
                }
                if !preview.samples.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("样例路径")
                            .size(11.0)
                            .color(theme::secondary_label(dark)),
                    );
                    for s in &preview.samples {
                        ui.label(
                            RichText::new(format!(
                                "{} → {}",
                                s.input
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?"),
                                s.output
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?")
                            ))
                            .size(11.0)
                            .family(egui::FontFamily::Monospace),
                        );
                    }
                }
            } else if !self.input_dir.trim().is_empty() {
                ui.label(
                    RichText::new("点击「刷新预估」查看将转换的文件数")
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                );
            } else {
                ui.label(
                    RichText::new("选择输入文件夹后，可在此预估批量规模")
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                );
            }

            if let Some(err) = &self.quality_preview_error {
                ui.colored_label(theme::error_color(dark), err);
            } else if !self.quality_preview_rows.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("单图质量对比（首图采样）")
                        .size(11.0)
                        .color(theme::secondary_label(dark)),
                );
                for row in &self.quality_preview_rows {
                    ui.label(format!(
                        "质量 {} → {}",
                        row.quality,
                        quality_preview::format_bytes(row.bytes)
                    ));
                }
            } else if self.quality_preview_worker.is_some() {
                ui.label("正在计算质量体积预览…");
            }
        });
    }

    fn convert_run_status_ui(&mut self, ui: &mut egui::Ui, dark: bool, running: bool) {
        use eframe::egui::RichText;
        use crate::gui::theme;

        if running {
            let bar_h = ui.spacing().interact_size.y;
            ui.add_sized(
                egui::vec2(ui.available_width(), bar_h),
                egui::ProgressBar::new(match &self.state {
                    RunState::Running { progress, .. } => progress.fraction(),
                    _ => 0.0,
                })
                .text("处理中…")
                .show_percentage()
                .animate(running),
            );
            ui.add_space(8.0);
        } else if let RunState::Done(report) = &self.state {
            let ratio = report.compression_ratio() * 100.0;
            ui.label(
                RichText::new(format!("压缩率约 {ratio:.1}%"))
                    .size(13.0)
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(8.0);
        }

        widgets::status_banner(ui, &self.status, running);
    }
}
