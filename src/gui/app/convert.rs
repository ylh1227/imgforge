//! 转换配置、预览、worker 与设置控件。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use eframe::egui;

use crate::config::AppConfig;
use crate::core::types::{ImageFormat, MetadataPolicy, Quality, ResizeOptions};
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
        self.refresh_previews();
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
        ui.add_space(4.0);
        widgets::checkbox_grid(ui, &mut file_options, enabled, 2);

        #[cfg(feature = "bayer")]
        {
            widgets::inset_separator(ui);
            widgets::settings_subheading(ui, "RAW 处理");
            ui.add_space(4.0);
            let mut raw_options = [(&mut self.bayer_only, "仅解 Bayer/RAW（不做缩放锐化）")];
            widgets::checkbox_grid(ui, &mut raw_options, enabled, 1);
        }

        widgets::inset_separator(ui);
        widgets::settings_subheading(ui, "远端执行");
        ui.add_space(4.0);
        ui.add_enabled_ui(enabled, |ui| {
            ui.checkbox(
                &mut self.prefer_remote_execution,
                "全模块远程任务（需配置 [remote]；失败不会自动回退本地）",
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

        const ROW_GAP: f32 = 6.0;
        const FIELD_GAP: f32 = 8.0;
        let field_h = widgets::TOOLBAR_ROW_HEIGHT;

        widgets::grouped_section(ui, "设备导入", |ui| {
            ui.label(
                RichText::new("从手机、运动相机等导入图片/视频到本地暂存目录，再用于转换或评审")
                    .size(12.0)
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(8.0);

            let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;

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
                if narrow {
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

            // 方式
            control_row(ui, "方式", &mut |ui| {
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
            });
            ui.add_space(ROW_GAP);

            // 来源
            let source_needs_browse = matches!(
                self.mobile_backend,
                MobilePullBackend::Fs | MobilePullBackend::Auto
            );
            let mut source_browse = false;
            let source_hint = match self.mobile_backend {
                MobilePullBackend::Fs => "选择设备挂载目录…",
                _ => "/sdcard/DCIM 或本地挂载路径",
            };
            control_row(ui, "来源", &mut |ui| {
                source_browse = widgets::path_field_fill(
                    ui,
                    &mut self.mobile_source,
                    source_hint,
                    enabled,
                    source_needs_browse,
                );
            });
            if source_browse {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.mobile_source = folder.display().to_string();
                    if matches!(self.mobile_backend, MobilePullBackend::Auto) {
                        self.mobile_backend = MobilePullBackend::Fs;
                    }
                }
            }
            ui.add_space(ROW_GAP);

            // 暂存
            let mut staging_browse = false;
            control_row(ui, "暂存", &mut |ui| {
                staging_browse = widgets::path_field_fill(
                    ui,
                    &mut self.mobile_staging,
                    "本地暂存目录…",
                    enabled,
                    true,
                );
            });
            if staging_browse {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.mobile_staging = folder.display().to_string();
                }
            }
            ui.add_space(ROW_GAP);

            // 设备
            if !matches!(self.mobile_backend, MobilePullBackend::Fs) {
                control_row(ui, "设备", &mut |ui| {
                    let _ = widgets::path_field_fill(
                        ui,
                        &mut self.mobile_adb_serial,
                        "多设备时填写 ADB serial，可留空",
                        enabled,
                        false,
                    );
                });
                ui.add_space(ROW_GAP);
            }

            // 导入按钮
            control_row(ui, "", &mut |ui| {
                let w = ui.available_width().max(80.0);
                if widgets::full_width_primary_button_in(ui, "从设备导入", enabled, w).clicked()
                {
                    self.start_device_import();
                }
            });
        });
    }

    pub(super) fn start_device_import(&mut self) {
        use crate::gui::app_types::DeviceImportResult;
        use crate::mobile::{import_media, MobilePullConfig};

        if self.is_running() {
            return;
        }
        if self.mobile_source.trim().is_empty() {
            self.status = "请填写设备来源路径".into();
            self.push_log(self.status.clone());
            return;
        }
        if self.mobile_staging.trim().is_empty() {
            self.status = "请填写本地暂存目录".into();
            self.push_log(self.status.clone());
            return;
        }

        let config = MobilePullConfig {
            enabled: true,
            backend: self.mobile_backend,
            source_path: self.mobile_source.trim().to_string(),
            staging_dir: PathBuf::from(self.mobile_staging.trim()),
            preserve_structure: true,
            adb_serial: {
                let s = self.mobile_adb_serial.trim();
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            },
            adb_mode: crate::mobile::AdbBinaryMode::Auto,
            adb_path: None,
            allow_path_fallback: true,
            delete_after_pull: false,
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
        self.push_log(format!(
            "设备导入：{:?} {} → {}",
            config.backend,
            config.source_path,
            config.staging_dir.display()
        ));

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
            widgets::settings_labeled_row(ui, "目标格式", |ui| {
                let combo_w = f32::min(140.0, ui.available_width());
                egui::ComboBox::from_id_salt("format")
                    .width(combo_w)
                    .selected_text(self.formats[self.format_index].extension().to_uppercase())
                    .show_ui(ui, |ui| {
                        for (idx, format) in self.formats.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.format_index,
                                idx,
                                format.extension().to_uppercase(),
                            );
                        }
                    });
            });

            ui.add_space(6.0);
            widgets::quality_slider_row(ui, &mut self.quality, enabled && !self.use_target_max_bytes);
            ui.add_space(6.0);
            widgets::quality_presets_row(
                ui,
                &mut self.quality,
                enabled && !self.use_target_max_bytes,
            );

            ui.add_space(6.0);
            widgets::settings_labeled_row(ui, "目标体积", |ui| {
                ui.checkbox(&mut self.use_target_max_bytes, "限制单文件 ≤");
                ui.add_enabled_ui(enabled && self.use_target_max_bytes, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.target_max_kb)
                            .range(16..=20_480)
                            .suffix(" KB"),
                    );
                });
            });
            if self.use_target_max_bytes {
                ui.label(
                    RichText::new("启用后将对 JPEG/WebP 等自动二分搜索质量以控制体积")
                        .size(11.0)
                        .color(theme::secondary_label(dark)),
                );
            }

            ui.add_space(6.0);
            widgets::settings_labeled_row(ui, "重命名", |ui| {
                let response = ui.add_enabled_ui(enabled, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.rename_template)
                            .desired_width(ui.available_width().min(280.0))
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
                ui.label(
                    RichText::new("预览输出名")
                        .size(11.0)
                        .color(theme::secondary_label(dark)),
                );
                for (src, out) in &self.rename_preview {
                    ui.label(
                        RichText::new(format!("{src} → {out}"))
                            .size(11.0)
                            .family(egui::FontFamily::Monospace),
                    );
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
            ui.horizontal_wrapped(|ui| {
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
