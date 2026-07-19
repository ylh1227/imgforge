//! ImgForge 图形界面：文件夹选择、格式设置、进度与结果展示。

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use eframe::egui::{self, ScrollArea};

use crate::core::types::{ImageFormat, Quality};
use crate::gui::prefs::GuiPrefs;
use crate::gui::quality_preview::{QualityPreviewWorker, QualitySizeRow};
use crate::gui::task_center::{self, TaskCenterAction};
use crate::gui::{fonts, native, theme, widgets};
use crate::io::batch_preview::BatchPreview;

use crate::gui::app_types::{AppMode, AppReviewHost, RunState, WorkerMessage};

/// 主窗口应用。
pub struct ImgforgeApp {
    mode: AppMode,
    review_panel: Option<crate::review::ui::ReviewPanel>,
    #[cfg(feature = "video-review")]
    video_review_panel: Option<crate::video_review::ui::VideoReviewPanel>,
    #[cfg(feature = "video-review")]
    video_review_init_error: Option<String>,
    #[cfg(feature = "data-extract")]
    data_extract_panel: Option<crate::data_extract::ui::DataExtractPanel>,
    review_queue: Vec<PathBuf>,
    review_queue_params: std::collections::HashMap<PathBuf, crate::config::ConvertOverride>,
    burn_review_annotations: bool,
    input_dir: String,
    output_dir: String,
    format_index: usize,
    formats: Vec<ImageFormat>,
    quality: u8,
    recursive: bool,
    preserve_structure: bool,
    overwrite: bool,
    strip_metadata: bool,
    bayer_only: bool,
    brightness_match_enabled: bool,
    brightness_match_mode: crate::core::types::BrightnessMatchMode,
    brightness_match_path: String,
    /// true = percentile, false = mean
    brightness_match_metric_percentile: bool,
    brightness_match_percentile: f32,
    brightness_match_regional: bool,
    /// 参考图预览纹理（path, texture）。
    brightness_match_preview: Option<(String, egui::TextureHandle)>,
    /// 转换设置高级分段 Tab（RAW / 亮度 / 远端 / JIRA）。
    convert_advanced_tab: usize,
    rename_template: String,
    use_target_max_bytes: bool,
    target_max_kb: u32,
    gui_prefs: GuiPrefs,
    selected_preset: Option<usize>,
    new_preset_name: String,
    batch_preview: Option<BatchPreview>,
    rename_preview: Vec<(String, String)>,
    rename_preview_error: Option<String>,
    quality_preview_rows: Vec<QualitySizeRow>,
    quality_preview_error: Option<String>,
    quality_preview_worker: Option<QualityPreviewWorker>,
    status: String,
    log_lines: Vec<String>,
    state: RunState,
    worker_rx: Option<Receiver<WorkerMessage>>,
    last_failed_inputs: Vec<PathBuf>,
    native_toolbar: Option<native::NativeGlassToolbar>,
    /// 远端配置（默认关闭；可从环境变量叠加）。
    remote_config: crate::remote::RemoteConfig,
    /// JIRA 批量提 Bug 配置。
    jira_config: crate::jira::JiraConfig,
    jira_probe_status: Option<String>,
    /// 是否优先远端执行（默认 false，仍走本地流水线）。
    prefer_remote_execution: bool,
    /// 最近一次远端同步快照。
    remote_snapshot: Option<crate::remote::SyncSnapshot>,
    /// 设备导入：后端（自动 / 本地挂载 / ADB）。
    mobile_backend: crate::mobile::MobilePullBackend,
    /// 设备端来源路径（ADB 为 Android 路径；挂载模式为本地目录）。
    mobile_source: String,
    /// 本地暂存目录。
    mobile_staging: String,
    /// 多设备时的 ADB serial 手动补充（可空；可逗号分隔）。
    mobile_adb_serial: String,
    /// 刷新得到的 ADB 设备：信息、勾选、每台独立来源路径。
    mobile_adb_devices: Vec<MobileAdbDeviceRow>,
    /// 是否已成功刷新过设备列表（用于区分「未指定」与「全不勾选」）。
    mobile_adb_devices_loaded: bool,
    /// 设备拉取并发（1–8）。
    mobile_pull_concurrency: usize,
}

/// GUI 中单台 ADB 设备行。
#[derive(Debug, Clone)]
struct MobileAdbDeviceRow {
    info: crate::mobile::AdbDeviceInfo,
    selected: bool,
    /// 该设备上的来源路径（可覆盖全局 mobile_source）。
    source_path: String,
    /// 该设备本地保存路径；空 = 使用全局暂存/<serial>。
    staging_path: String,
}

mod convert;

impl ImgforgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        fonts::install_cjk_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);

        let formats = ImageFormat::all_supported();
        let review_panel = crate::review::ui::ReviewPanel::new().ok();
        #[cfg(feature = "video-review")]
        let (video_review_panel, video_review_init_error) =
            match crate::video_review::ui::VideoReviewPanel::new() {
                Ok(panel) => (Some(panel), None),
                Err(e) => (None, Some(e)),
            };
        #[cfg(feature = "data-extract")]
        let data_extract_panel = Some(crate::data_extract::ui::DataExtractPanel::new());
        let gui_prefs = GuiPrefs::load();
        let jira_config = crate::jira::load_jira_config_with_prefs(Some(&gui_prefs.jira));
        let bm = gui_prefs.brightness_match.clone();
        Self {
            mode: AppMode::Convert,
            review_panel,
            #[cfg(feature = "video-review")]
            video_review_panel,
            #[cfg(feature = "video-review")]
            video_review_init_error,
            #[cfg(feature = "data-extract")]
            data_extract_panel,
            review_queue: Vec::new(),
            review_queue_params: std::collections::HashMap::new(),
            burn_review_annotations: false,
            input_dir: String::new(),
            output_dir: String::from("./output"),
            format_index: formats
                .iter()
                .position(|f| *f == ImageFormat::WebP)
                .unwrap_or(0),
            formats,
            quality: Quality::DEFAULT.value(),
            recursive: true,
            preserve_structure: true,
            overwrite: false,
            strip_metadata: false,
            bayer_only: false,
            brightness_match_enabled: bm.enabled,
            brightness_match_mode: bm.mode,
            brightness_match_path: bm.path,
            brightness_match_metric_percentile: bm.metric_percentile,
            brightness_match_percentile: bm.percentile.clamp(90.0, 99.0),
            brightness_match_regional: bm.regional,
            brightness_match_preview: None,
            convert_advanced_tab: 0,
            rename_template: String::new(),
            use_target_max_bytes: false,
            target_max_kb: 500,
            gui_prefs,
            selected_preset: None,
            new_preset_name: String::new(),
            batch_preview: None,
            rename_preview: Vec::new(),
            rename_preview_error: None,
            quality_preview_rows: Vec::new(),
            quality_preview_error: None,
            quality_preview_worker: None,
            status: String::from("选择输入文件夹，然后点击「开始转换」"),
            log_lines: Vec::new(),
            state: RunState::Idle,
            worker_rx: None,
            last_failed_inputs: Vec::new(),
            native_toolbar: None,
            remote_config: {
                let mut remote = crate::remote::RemoteConfig::default();
                remote.apply_env_overrides();
                remote
            },
            jira_config,
            jira_probe_status: None,
            prefer_remote_execution: false,
            remote_snapshot: None,
            mobile_backend: crate::mobile::MobilePullBackend::Auto,
            mobile_source: String::from("/sdcard/DCIM"),
            mobile_staging: String::from(".imgforge/mobile-import"),
            mobile_adb_serial: String::new(),
            mobile_adb_devices: Vec::new(),
            mobile_adb_devices_loaded: false,
            mobile_pull_concurrency: crate::mobile::MOBILE_PULL_CONCURRENCY_DEFAULT,
        }
    }

    fn is_running(&self) -> bool {
        matches!(self.state, RunState::Running { .. })
    }

    fn push_log(&mut self, line: impl Into<String>) {
        self.log_lines.push(line.into());
        if self.log_lines.len() > 200 {
            let drain = self.log_lines.len() - 200;
            self.log_lines.drain(0..drain);
        }
    }
}

impl eframe::App for ImgforgeApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll_worker();
        self.poll_quality_preview();

        let running = self.is_running();
        let enabled = !running;

        if let RunState::Running { progress, .. } = &self.state {
            if let Some(label) = progress.status_label() {
                self.status = label;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        ctx.input(|input| {
            if enabled && !input.raw.dropped_files.is_empty() {
                for file in &input.raw.dropped_files {
                    if let Some(path) = &file.path {
                        if path.is_dir() {
                            self.input_dir = path.display().to_string();
                            self.status = format!("已拖入文件夹：{}", path.display());
                            self.refresh_previews();
                            break;
                        }
                    }
                }
            }
        });

        let dark = ctx.style().visuals.dark_mode;

        if self.native_toolbar.is_none() {
            self.native_toolbar = native::NativeGlassToolbar::try_install(frame);
        }

        let native_toolbar_active = self
            .native_toolbar
            .as_ref()
            .is_some_and(|toolbar| toolbar.is_active());

        if let Some(toolbar) = &mut self.native_toolbar {
            toolbar.sync(enabled, running);
            for action in toolbar.drain_actions() {
                match action {
                    native::ToolbarAction::Start => self.start_conversion(),
                    native::ToolbarAction::Cancel => self.cancel_conversion(),
                    native::ToolbarAction::OpenOutput => self.open_output_folder(),
                }
            }
        }

        if self.mode == AppMode::Convert && !native_toolbar_active {
            egui::TopBottomPanel::bottom("action_toolbar")
                .exact_height(64.0)
                .frame(widgets::glass_toolbar_frame(dark))
                .show(ctx, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(click) = widgets::action_toolbar_row(ui, enabled, running) {
                        match click {
                            widgets::ToolbarClick::Start => self.start_conversion(),
                            widgets::ToolbarClick::Cancel => self.cancel_conversion(),
                            widgets::ToolbarClick::OpenOutput => self.open_output_folder(),
                        }
                    }
                });
        }

        let viewport = theme::viewport_size(ctx);
        let content_w = theme::content_width(viewport.x);
        let bottom_reserve = if native_toolbar_active {
            native::TOOLBAR_HEIGHT + 14.0
        } else {
            88.0
        };
        let log_height = theme::log_panel_height(viewport.y, bottom_reserve);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::window_fill(dark)))
            .show(ctx, |ui| {
                ui.add_space(theme::macos_titlebar_inset(ctx));

                // 顶栏左上：品牌；其下 Tab 与内容列同宽左对齐
                widgets::content_column(ui, content_w, |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        widgets::brand_mark(ui);
                    });
                    ui.add_space(6.0);
                    let show_tabs = self.review_panel.is_some()
                        || cfg!(feature = "video-review")
                        || cfg!(feature = "data-extract");
                    if show_tabs {
                        let mut tabs = vec![(AppMode::Convert, "格式转换")];
                        if self.review_panel.is_some() {
                            tabs.push((AppMode::Review, "图片评审"));
                        }
                        #[cfg(feature = "video-review")]
                        tabs.push((AppMode::VideoReview, "视频评审"));
                        #[cfg(feature = "data-extract")]
                        tabs.push((AppMode::DataExtract, "数据提取"));
                        tabs.push((AppMode::TaskCenter, "任务中心"));
                        widgets::mode_tab_bar(ui, &mut self.mode, &tabs);
                    }
                });
                widgets::chrome_gap(ui);

                if self.mode == AppMode::TaskCenter {
                    let min_h = ui.available_height();
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_height(min_h);
                            widgets::content_column(ui, content_w, |ui| {
                                if let Some(action) = task_center::task_center_ui(
                                    ui,
                                    &self.gui_prefs,
                                    &self.last_failed_inputs,
                                    &task_center::RemoteTaskCenterView::from_config(
                                        &self.remote_config,
                                        self.prefer_remote_execution,
                                        self.remote_snapshot.as_ref(),
                                    ),
                                    enabled,
                                ) {
                                    match action {
                                        TaskCenterAction::LoadConvertHistory(idx) => {
                                            if let Some(entry) =
                                                self.gui_prefs.history.get(idx).cloned()
                                            {
                                                self.input_dir = entry.input_dir;
                                                self.output_dir = entry.output_dir;
                                                self.apply_snapshot(&entry.snapshot);
                                                self.mode = AppMode::Convert;
                                                self.status =
                                                    format!("已从任务中心载入转换历史 #{idx}");
                                            }
                                        }
                                        TaskCenterAction::RetryConvertFailures(paths) => {
                                            self.retry_convert_failures(paths);
                                        }
                                        TaskCenterAction::SyncRemoteJobs => {
                                            self.sync_remote_jobs();
                                        }
                                        TaskCenterAction::RefreshRemoteJob(job_id) => {
                                            self.refresh_remote_job(&job_id);
                                        }
                                        TaskCenterAction::OpenReviewRemote => {
                                            if let Some(panel) = &mut self.review_panel {
                                                panel.set_remote_config(self.remote_config.clone());
                                                panel.refresh_remote_catalog();
                                                self.mode = AppMode::Review;
                                                self.status = "已切换到远程图片评审".into();
                                            } else {
                                                self.status = "图片评审模块不可用".into();
                                            }
                                        }
                                        TaskCenterAction::OpenVideoRemote => {
                                            #[cfg(feature = "video-review")]
                                            if let Some(panel) = &mut self.video_review_panel {
                                                panel.set_remote_config(self.remote_config.clone());
                                                panel.refresh_remote_catalog();
                                                self.mode = AppMode::VideoReview;
                                                self.status = "已切换到远程视频评审".into();
                                            } else {
                                                self.status = "视频评审模块不可用".into();
                                            }
                                            #[cfg(not(feature = "video-review"))]
                                            {
                                                self.status = "视频评审未编译".into();
                                            }
                                        }
                                        TaskCenterAction::OpenExtractRemote => {
                                            #[cfg(feature = "data-extract")]
                                            if let Some(panel) = &mut self.data_extract_panel {
                                                panel.set_remote_config(self.remote_config.clone());
                                                panel.refresh_remote_catalog();
                                                self.mode = AppMode::DataExtract;
                                                self.status = "已切换到远程数据提取结果".into();
                                            } else {
                                                self.status = "数据提取模块不可用".into();
                                            }
                                            #[cfg(not(feature = "data-extract"))]
                                            {
                                                self.status = "数据提取未编译".into();
                                            }
                                        }
                                    }
                                }
                            });
                        });
                    return;
                }

                if self.mode == AppMode::Review {
                    if let Some(panel) = &mut self.review_panel {
                        panel.set_remote_config(self.remote_config.clone());
                        panel.set_jira_config(self.jira_config.clone());
                        let host = AppReviewHost {
                            queue: &self.review_queue,
                            output_dir: &self.output_dir,
                        };
                        // 评审页自行分配「顶栏 + 剩余正文」高度；避免外层 ScrollArea
                        // 与三栏定高嵌套导致左右栏底部裁切、内滚动失效。
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                right: 12,
                                bottom: 12,
                                left: 8,
                                ..egui::Margin::ZERO
                            })
                            .show(ui, |ui| {
                                panel.ui(ctx, ui, &host);
                            });
                        let output = panel.take_output();
                        if !output.enqueue_approved.is_empty() {
                            self.review_queue = output.enqueue_approved;
                            self.review_queue_params = output
                                .enqueue_params
                                .iter()
                                .filter(|i| !i.params.is_empty())
                                .map(|i| {
                                    (
                                        i.path.clone(),
                                        crate::config::ConvertOverride {
                                            format: i.params.format,
                                            quality: i.params.quality.and_then(|q| {
                                                crate::core::types::Quality::new(q).ok()
                                            }),
                                            width: i.params.width,
                                        },
                                    )
                                })
                                .collect();
                            self.mode = AppMode::Convert;
                            self.status = format!(
                                "已从评审导入 {} 张「通过」图片，可开始转换",
                                self.review_queue.len()
                            );
                            self.push_log(self.status.clone());
                        } else if output.switch_to_convert {
                            self.mode = AppMode::Convert;
                            self.status = output.status_message.clone();
                        } else if !output.status_message.is_empty() {
                            self.status = output.status_message;
                        }
                    } else {
                        ui.label("评审模块初始化失败");
                    }
                    return;
                }

                if self.mode == AppMode::VideoReview {
                    #[cfg(feature = "video-review")]
                    if let Some(panel) = &mut self.video_review_panel {
                        panel.set_remote_config(self.remote_config.clone());
                        panel.set_jira_config(self.jira_config.clone());
                        let min_h = ui.available_height();
                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_height(min_h);
                                egui::Frame::new()
                                    .inner_margin(egui::Margin {
                                        right: 28,
                                        ..egui::Margin::ZERO
                                    })
                                    .show(ui, |ui| {
                                        panel.ui(ctx, ui);
                                    });
                            });
                        let output = panel.take_output();
                        if !output.status_message.is_empty() {
                            self.status = output.status_message;
                        }
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            widgets::navigation_header(ui, "视频评审");
                            ui.add_space(12.0);
                            let msg = self
                                .video_review_init_error
                                .as_deref()
                                .unwrap_or("视频评审模块初始化失败");
                            widgets::warning_banner(ui, msg);
                        });
                    }
                    #[cfg(not(feature = "video-review"))]
                    ui.label("视频评审未编译（需启用 video-review feature）");
                    return;
                }

                if self.mode == AppMode::DataExtract {
                    #[cfg(feature = "data-extract")]
                    if let Some(panel) = &mut self.data_extract_panel {
                        panel.set_remote_config(self.remote_config.clone());
                        let min_h = ui.available_height();
                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_height(min_h);
                                egui::Frame::new()
                                    .inner_margin(egui::Margin {
                                        right: 28,
                                        ..egui::Margin::ZERO
                                    })
                                    .show(ui, |ui| {
                                        panel.ui(ctx, ui);
                                    });
                            });
                        let output = panel.take_output();
                        if !output.status_message.is_empty() {
                            self.status = output.status_message;
                        }
                    } else {
                        ui.label("数据提取模块初始化失败");
                    }
                    #[cfg(not(feature = "data-extract"))]
                    ui.label("数据提取未编译（需启用 data-extract feature）");
                    return;
                }

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        widgets::content_column(ui, content_w, |ui| {
                            self.convert_page_ui(
                                ui,
                                dark,
                                enabled,
                                running,
                                log_height,
                                bottom_reserve,
                            );
                        });
                    });
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.gui_prefs.jira = crate::jira::JiraPrefsSnapshot::from_config(&self.jira_config);
        let _ = self.gui_prefs.save();
        if let Some(toolbar) = &mut self.native_toolbar {
            toolbar.teardown();
        }
    }
}
