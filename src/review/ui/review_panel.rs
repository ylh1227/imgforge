//! 评审主面板：三栏布局，串联标注画布、对比视图、批量操作与转换队列联动。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::{self, RichText, ScrollArea};

use crate::gui::prefs::{self, ActionHistoryEntry, ActionHistoryStatus, GuiPrefs};
use crate::gui::theme;
use crate::gui::widgets;
use crate::gui::BackgroundJob;
use crate::remote::DataSource;
use crate::review::domain::annotation::{
    Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle, ArrowPosition,
    RectanglePosition, TextPosition,
};
use crate::review::domain::image_item::next_image_id;
use crate::review::domain::{BatchStats, ReviewBatch, ReviewImageItem, ReviewStatus};
use crate::review::error::ReviewResult;
use crate::review::is_irreversible_transition;
use crate::review::service::{
    BatchAnnotateRequest, BatchImageScreenshotRequest, BatchImageScreenshotResult,
    BatchImageScreenshotService, BatchRemarkRequest, BatchStatusRequest, ExportService,
    ShortcutAction, StatusTransitionWarning,
};
use crate::review::service::{ImageAnalysis, ImageAnalysisService, ReviewModuleConfig};
use crate::review::storage::SqliteReviewRepository;
use crate::review::ui::annotation_canvas::AnnotationCanvasEvent;
use crate::review::ui::compare_view::{CompareDisplayMode, CompareView, MAX_MULTI_COMPARE_PANES};
use crate::review::ui::properties_panel::{properties_panel_ui, PropertiesPanelState};
use crate::review::ui::shortcut_panel::{shortcut_panel_ui, ShortcutPanelState};
use crate::review::ui::shortcuts::handle_shortcuts;
use crate::review::ui::sidebar::{
    batch_list_ui, filter_sort_ui, format_stats, image_list_body_ui, image_list_ui, status_buttons,
    SidebarState,
};
use crate::review::ui::ListThumbnailCache;
use crate::review::RemarkWriteMode;
use crate::review::{ReviewConversionBridge, ReviewService};
use crate::ui::progress::ProgressReporter;
use crate::video_review::service::ScreenshotFormat;

use crate::review::ui::review_panel_helpers::{
    annotation_kind_label, batch_op_description, file_mtime_key, format_contact_sheets,
    histogram_ui, truncate_text, viewport_size,
};

use crate::review::ui::review_panel_types::{BatchOpKind, DialogState, RightTab};
pub use crate::review::ui::review_panel_types::{ReviewPanelHost, ReviewPanelOutput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewLayoutMode {
    ThreeColumn,
    TwoColumn,
    Stacked,
}

impl ReviewLayoutMode {
    fn from_width(width: f32) -> Self {
        if width >= theme::REVIEW_THREE_COL_BREAKPOINT {
            Self::ThreeColumn
        } else if width >= theme::REVIEW_TWO_COL_BREAKPOINT {
            Self::TwoColumn
        } else {
            Self::Stacked
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ReviewStackPane {
    #[default]
    List,
    Canvas,
    Inspector,
}

/// egui 评审主面板（独立 Tab 入口）。
pub struct ReviewPanel {
    service: ReviewService,
    batches: Vec<ReviewBatch>,
    batch_stats: Vec<(i64, BatchStats)>,
    images: Vec<ReviewImageItem>,
    current_batch: Option<i64>,
    current_image: Option<i64>,
    compare_view: CompareView,
    current_annotations: Vec<Annotation>,
    sidebar: SidebarState,
    converted_preview: Option<PathBuf>,
    remark_buf: String,
    output: ReviewPanelOutput,
    error: Option<String>,
    status_hint: String,
    pending_import: Option<(Vec<PathBuf>, String)>,
    dialog: Option<DialogState>,
    batch_target_status: ReviewStatus,
    batch_remark_mode: RemarkWriteMode,
    last_batch_annotation_ids: Vec<i64>,
    config: ReviewModuleConfig,
    remote_config: crate::remote::RemoteConfig,
    remote_batch_id: Option<String>,
    remote_item_ids: HashMap<i64, String>,
    data_source: crate::remote::DataSource,
    remote_batches: Vec<crate::remote::RemoteReviewBatchSummary>,
    remote_items: Vec<crate::remote::RemoteReviewItem>,
    remote_id_map: crate::remote::RemoteIdMap,
    batches_fetch: Option<crate::remote::RemoteFetch<Vec<crate::remote::RemoteReviewBatchSummary>>>,
    items_fetch:
        Option<crate::remote::RemoteFetch<Vec<(crate::remote::RemoteReviewItem, Option<PathBuf>)>>>,
    asset_fetch: Option<crate::remote::RemoteFetch<(i64, PathBuf)>>,
    remote_loading: bool,
    pending_open_remote_batch_id: Option<String>,
    properties: PropertiesPanelState,
    shortcut_panel: ShortcutPanelState,
    show_shortcut_panel: bool,
    last_backup_minute: u64,
    right_tab: RightTab,
    /// 小窗堆叠布局当前分段。
    stack_pane: ReviewStackPane,
    all_tags: Vec<crate::review::ReviewTag>,
    current_image_tags: Vec<i64>,
    new_tag_name: String,
    new_tag_color_idx: usize,
    renaming_tag: Option<(i64, String)>,
    list_thumbs: ListThumbnailCache,
    analysis_cache: HashMap<PathBuf, (Option<u64>, ImageAnalysis)>,
    current_analysis: Option<ImageAnalysis>,
    analysis_error: Option<String>,
    screenshot_include_annotations: bool,
    screenshot_use_all_visible: bool,
    screenshot_format: ScreenshotFormat,
    screenshot_write_json: bool,
    screenshot_write_contact_sheet: bool,
    screenshot_job: BackgroundJob<BatchImageScreenshotResult>,
    screenshot_job_started: Option<Instant>,
    screenshot_job_dir: Option<PathBuf>,
    /// 画布区域最近一次布局尺寸（用于「适应窗口」，避免用整窗 viewport 误算）。
    canvas_area_size: egui::Vec2,
    last_viewport_size: egui::Vec2,
    viewport_resize_frames: u8,
}

impl ReviewPanel {
    pub fn new() -> ReviewResult<Self> {
        let service = ReviewService::open()?;
        let shortcuts = service.shortcuts.clone();
        let config = ReviewModuleConfig::load().unwrap_or_default();
        let mut remote_config = crate::remote::RemoteConfig::default();
        remote_config.apply_env_overrides();
        let data_source =
            DataSource::from_remote_enabled(crate::remote::remote_enabled(&remote_config));
        let mut panel = Self {
            service,
            batches: Vec::new(),
            batch_stats: Vec::new(),
            images: Vec::new(),
            current_batch: None,
            current_image: None,
            compare_view: CompareView::new(),
            current_annotations: Vec::new(),
            sidebar: SidebarState::default(),
            converted_preview: None,
            remark_buf: String::new(),
            output: ReviewPanelOutput::default(),
            error: None,
            status_hint: String::new(),
            pending_import: None,
            dialog: None,
            batch_target_status: ReviewStatus::Approved,
            batch_remark_mode: RemarkWriteMode::Overwrite,
            last_batch_annotation_ids: Vec::new(),
            config,
            remote_config,
            remote_batch_id: None,
            remote_item_ids: HashMap::new(),
            data_source,
            remote_batches: Vec::new(),
            remote_items: Vec::new(),
            remote_id_map: crate::remote::RemoteIdMap::new(),
            batches_fetch: None,
            items_fetch: None,
            asset_fetch: None,
            remote_loading: false,
            pending_open_remote_batch_id: None,
            properties: PropertiesPanelState::default(),
            shortcut_panel: ShortcutPanelState::new(&shortcuts),
            show_shortcut_panel: false,
            last_backup_minute: 0,
            right_tab: RightTab::default(),
            stack_pane: ReviewStackPane::default(),
            all_tags: Vec::new(),
            current_image_tags: Vec::new(),
            new_tag_name: String::new(),
            new_tag_color_idx: 0,
            renaming_tag: None,
            list_thumbs: ListThumbnailCache::default(),
            analysis_cache: HashMap::new(),
            current_analysis: None,
            analysis_error: None,
            screenshot_include_annotations: false,
            screenshot_use_all_visible: false,
            screenshot_format: ScreenshotFormat::Jpeg,
            screenshot_write_json: false,
            screenshot_write_contact_sheet: false,
            screenshot_job: BackgroundJob::default(),
            screenshot_job_started: None,
            screenshot_job_dir: None,
            canvas_area_size: egui::Vec2::ZERO,
            last_viewport_size: egui::Vec2::ZERO,
            viewport_resize_frames: 0,
        };
        let _ = panel.reload_tags();
        panel.reload_batches()?;
        if panel.data_source == DataSource::Local {
            if let Ok((batch, image)) = panel.service.restore_session() {
                panel.current_batch = batch;
                panel.current_image = image;
                let _ = panel.reload_images();
                panel.load_current_annotations();
            }
        }
        Ok(panel)
    }

    pub fn set_remote_config(&mut self, remote_config: crate::remote::RemoteConfig) {
        self.remote_config = remote_config;
        let want =
            DataSource::from_remote_enabled(crate::remote::remote_enabled(&self.remote_config));
        if self.data_source != want {
            self.data_source = want;
            let _ = self.reload_batches();
        }
    }

    /// 由主应用在切换 Tab 前调度：从转换队列创建评审批次。
    pub fn schedule_import_from_queue(
        &mut self,
        paths: Vec<PathBuf>,
        batch_name: impl Into<String>,
    ) {
        if !paths.is_empty() {
            self.pending_import = Some((paths, batch_name.into()));
        }
    }

    pub fn take_output(&mut self) -> ReviewPanelOutput {
        std::mem::take(&mut self.output)
    }

    pub fn open_remote_batch(&mut self, batch_id: &str) {
        self.data_source = DataSource::Remote;
        self.remote_batch_id = Some(batch_id.to_string());
        self.pending_open_remote_batch_id = Some(batch_id.to_string());
        self.start_remote_batches_fetch();
    }

    pub fn refresh_remote_catalog(&mut self) {
        self.data_source = DataSource::Remote;
        self.pending_open_remote_batch_id = None;
        self.start_remote_batches_fetch();
    }

    /// 渲染评审面板（三栏 + 顶栏 + 底栏）。
    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, host: &dyn ReviewPanelHost) {
        self.poll_remote_fetches(ctx);
        self.process_pending_import();
        self.handle_shortcut_actions(ctx);
        self.poll_screenshot_job(ctx);
        let vp = viewport_size(ctx);
        if (vp - self.last_viewport_size).length_sq() > 4.0 {
            self.viewport_resize_frames = 12;
            self.last_viewport_size = vp;
        } else if self.viewport_resize_frames > 0 {
            self.viewport_resize_frames -= 1;
        }
        self.compare_view
            .set_defer_texture_load(self.viewport_resize_frames > 0);
        // 切换对比模式下：空格键手动翻转原图/转换后
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.compare_view.toggle_flip();
        }
        self.show_dialogs(ctx);
        self.maybe_scheduled_backup();

        let dark = ui.style().visuals.dark_mode;
        let layout = ReviewLayoutMode::from_width(ui.available_width());
        let viewport_h = ui.available_height();
        // 小窗/矮视口：整页滚动（顶栏+正文一起滚），避免顶栏占满导致正文高度为 0
        // 高屏三栏：顶栏固定，正文吃满剩余高度
        let page_scroll = !matches!(layout, ReviewLayoutMode::ThreeColumn) || viewport_h < 780.0;

        if page_scroll {
            egui::ScrollArea::vertical()
                .id_salt("review_page_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let page_w = ui
                        .available_width()
                        .min(ui.max_rect().width())
                        .max(200.0);
                    ui.set_max_width(page_w);
                    ui.set_width(page_w);
                    self.paint_chrome(ui, ctx, host, dark, true);
                    match layout {
                        ReviewLayoutMode::ThreeColumn => {
                            let row_h = 520.0_f32.min(viewport_h.max(360.0)).max(360.0);
                            let body_w = ui.available_width();
                            ui.allocate_ui_with_layout(
                                egui::vec2(body_w, row_h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(body_w);
                                    ui.set_min_height(row_h);
                                    ui.set_max_height(row_h);
                                    self.layout_three_column(ui, ctx, host, dark);
                                },
                            );
                        }
                        ReviewLayoutMode::TwoColumn => {
                            self.layout_two_column(ui, ctx, host, dark);
                        }
                        ReviewLayoutMode::Stacked => {
                            self.layout_stacked(ui, ctx, host, dark);
                        }
                    }
                    ui.add_space(28.0);
                });
        } else {
            self.paint_chrome(ui, ctx, host, dark, false);
            let body_h = ui.available_height().max(theme::REVIEW_MIN_BODY_H);
            let body_w = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(body_w, body_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_height(body_h);
                    ui.set_max_height(body_h);
                    ui.set_width(body_w);
                    self.layout_three_column(ui, ctx, host, dark);
                },
            );
        }
    }

    fn paint_chrome(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        host: &dyn ReviewPanelHost,
        dark: bool,
        compact: bool,
    ) {
        if compact {
            ui.label(
                RichText::new("ImgForge")
                    .font(theme::title_font())
                    .strong()
                    .color(theme::primary_label(dark)),
            );
            ui.label(
                RichText::new("批注、对比与通过状态")
                    .font(theme::subtitle_font())
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(8.0);
        } else {
            widgets::navigation_header(ui, "批注、对比与通过状态");
            widgets::page_header_gap(ui);
        }

        widgets::grouped_section(ui, "操作", |ui| {
            self.top_toolbar(ui, host);
        });

        ui.add_space(if compact { 8.0 } else { theme::SECTION_GAP });

        if let Some(err) = &self.error {
            widgets::error_banner(ui, err);
            ui.add_space(6.0);
        }

        widgets::status_banner(ui, &self.status_message(dark), false);
        ui.add_space(6.0);

        self.main_workflow_bar(ui, ctx, dark);
        ui.add_space(if compact { 8.0 } else { theme::SECTION_GAP });

        if self.show_shortcut_panel {
            widgets::grouped_section(ui, "快捷键", |ui| {
                if shortcut_panel_ui(ui, &mut self.shortcut_panel) {
                    let _ = self.service.save_shortcuts(&self.shortcut_panel.draft);
                    self.set_status("快捷键已更新");
                }
            });
            ui.add_space(8.0);
        }
    }

    fn status_message(&self, dark: bool) -> String {
        let _ = dark;
        if !self.output.status_message.is_empty() {
            return self.output.status_message.clone();
        }
        if !self.status_hint.is_empty() {
            return self.status_hint.clone();
        }
        if let (Some(item), Some(idx)) = (self.current_item(), self.current_index()) {
            return format!(
                "就绪 · {}/{} · {}",
                idx + 1,
                self.images.len(),
                item.file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("—")
            );
        }
        "选择评审批次与图片，或从转换队列导入".into()
    }

    fn top_toolbar(&mut self, ui: &mut egui::Ui, host: &dyn ReviewPanelHost) {
        let avail = ui.available_width();
        ui.set_width(avail);
        let narrow = avail < theme::REVIEW_TWO_COL_BREAKPOINT;
        let queue_len = host.conversion_queue_paths().len();

        if narrow {
            if widgets::full_width_primary_button(ui, "从文件夹创建", true).clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.create_batch_from_folder(&folder);
                }
            }
            ui.add_space(6.0);
            let gap = 6.0;
            let cell = ((ui.available_width() - gap) * 0.5).max(100.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if widgets::full_width_secondary_button_in(
                    ui,
                    &format!("队列导入 ({queue_len})"),
                    queue_len > 0,
                    cell,
                )
                .clicked()
                {
                    let paths = host.conversion_queue_paths().to_vec();
                    self.import_from_paths(&paths, "转换队列导入");
                }
                if widgets::full_width_secondary_button_in(
                    ui,
                    "导出 CSV",
                    self.current_batch.is_some(),
                    cell,
                )
                .clicked()
                {
                    self.export_csv();
                }
            });
            ui.add_space(gap);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if widgets::full_width_secondary_button_in(
                    ui,
                    "导出标注",
                    self.current_image.is_some(),
                    cell,
                )
                .clicked()
                {
                    self.export_sidecar();
                }
                if widgets::full_width_secondary_button_in(
                    ui,
                    "批量 JSON",
                    self.current_batch.is_some(),
                    cell,
                )
                .clicked()
                {
                    self.export_batch_json();
                }
            });
            ui.add_space(gap);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if widgets::full_width_secondary_button_in(ui, "快捷键", true, cell).clicked() {
                    self.show_shortcut_panel = !self.show_shortcut_panel;
                }
                if widgets::full_width_secondary_button_in(ui, "备份数据", true, cell).clicked() {
                    match crate::review::storage::create_backup() {
                        Ok(p) => self.set_status(format!("已备份：{}", p.display())),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            });
            ui.add_space(gap);
            if widgets::full_width_secondary_button(ui, "清理缩略图缓存", true).clicked() {
                match crate::review::service::ThumbnailService::clear_cache() {
                    Ok(n) => {
                        self.list_thumbs.clear();
                        self.set_status(format!("已清理 {n} 个缩略图缓存"));
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            if crate::remote::remote_enabled(&self.remote_config) {
                ui.add_space(gap);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    ui.label("数据源");
                    if ui
                        .selectable_label(self.data_source == DataSource::Remote, "远程")
                        .clicked()
                        && self.data_source != DataSource::Remote
                    {
                        self.data_source = DataSource::Remote;
                        self.start_remote_batches_fetch();
                    }
                    if ui
                        .selectable_label(self.data_source == DataSource::Local, "本地")
                        .clicked()
                        && self.data_source != DataSource::Local
                    {
                        self.switch_to_local("已切换到本地数据源");
                    }
                    if self.remote_loading {
                        ui.spinner();
                    }
                    if widgets::compact_secondary_button(
                        ui,
                        "刷新远程",
                        self.data_source == DataSource::Remote && !self.remote_loading,
                    )
                    .clicked()
                    {
                        self.start_remote_batches_fetch();
                    }
                });
            }
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(avail);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            if widgets::compact_primary_button(ui, "从文件夹创建", true).clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.create_batch_from_folder(&folder);
                }
            }
            if crate::remote::remote_enabled(&self.remote_config) {
                ui.label("数据源");
                if ui
                    .selectable_label(self.data_source == DataSource::Remote, "远程")
                    .clicked()
                    && self.data_source != DataSource::Remote
                {
                    self.data_source = DataSource::Remote;
                    self.start_remote_batches_fetch();
                }
                if ui
                    .selectable_label(self.data_source == DataSource::Local, "本地")
                    .clicked()
                    && self.data_source != DataSource::Local
                {
                    self.switch_to_local("已切换到本地数据源");
                }
                if self.remote_loading {
                    ui.spinner();
                    ui.label("加载中…");
                }
                if widgets::compact_secondary_button(
                    ui,
                    "刷新远程",
                    self.data_source == DataSource::Remote && !self.remote_loading,
                )
                .clicked()
                {
                    self.start_remote_batches_fetch();
                }
            }

            if widgets::compact_secondary_button(
                ui,
                &format!("从转换队列导入 ({queue_len})"),
                queue_len > 0,
            )
            .clicked()
            {
                let paths = host.conversion_queue_paths().to_vec();
                self.import_from_paths(&paths, "转换队列导入");
            }

            if widgets::compact_secondary_button(ui, "导出 CSV", self.current_batch.is_some())
                .clicked()
            {
                self.export_csv();
            }
            if widgets::compact_secondary_button(ui, "导出标注 JSON", self.current_image.is_some())
                .clicked()
            {
                self.export_sidecar();
            }
            if widgets::compact_secondary_button(ui, "批量导出 JSON", self.current_batch.is_some())
                .clicked()
            {
                self.export_batch_json();
            }
            if widgets::compact_secondary_button(ui, "快捷键", true).clicked() {
                self.show_shortcut_panel = !self.show_shortcut_panel;
            }
            if widgets::compact_secondary_button(ui, "备份数据库", true).clicked() {
                match crate::review::storage::create_backup() {
                    Ok(p) => self.set_status(format!("已备份：{}", p.display())),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            if widgets::compact_secondary_button(ui, "清理缩略图缓存", true).clicked() {
                match crate::review::service::ThumbnailService::clear_cache() {
                    Ok(n) => {
                        self.list_thumbs.clear();
                        self.set_status(format!("已清理 {n} 个缩略图缓存"));
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        });
    }

    /// 主界面常用工作条：导航、状态、筛选、视图与回流。
    fn main_workflow_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, dark: bool) {
        let has_image = self.current_image.is_some();
        let idx = self.current_index();
        let can_prev = idx.map(|i| i > 0).unwrap_or(false);
        let can_next = idx.map(|i| i + 1 < self.images.len()).unwrap_or(false);

        widgets::grouped_section(ui, "常用", |ui| {
            let avail = ui.available_width();
            ui.set_width(avail);
            let selected_count = self.sidebar.selected_ids.len();
            let page_label = idx.map(|i| format!("{}/{}", i + 1, self.images.len()));
            let narrow = avail < theme::REVIEW_TWO_COL_BREAKPOINT;

            // 导航 + 状态：始终可换行，避免小窗单行挤压
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(avail);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                if widgets::compact_secondary_button(ui, "◀ 上一张", can_prev).clicked() {
                    self.select_relative(-1);
                }
                if widgets::compact_secondary_button(ui, "下一张 ▶", can_next).clicked() {
                    self.select_relative(1);
                }
                if let Some(label) = &page_label {
                    ui.label(
                        RichText::new(label)
                            .size(13.0)
                            .color(theme::secondary_label(dark)),
                    );
                }
                let current_status = self.current_item().map(|item| item.status);
                if let Some(status) = status_buttons(ui, current_status) {
                    if let Some(id) = self.current_image {
                        self.set_image_status(id, status);
                    }
                }
            });

            ui.add_space(8.0);

            if narrow {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(avail);
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    widgets::toolbar_field_label(ui, "对比", dark);
                    self.compare_view.mode_selector_ui(ui);
                });
                ui.add_space(6.0);
                let batch_label = format!("批量对比 ({selected_count})");
                let enabled = selected_count >= 2;
                let clicked = if enabled {
                    widgets::full_width_primary_button(ui, &batch_label, true).clicked()
                } else {
                    widgets::full_width_secondary_button(ui, &batch_label, false).clicked()
                };
                if clicked {
                    self.start_batch_compare();
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(avail);
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    widgets::toolbar_field_label(ui, "对比模式", dark);
                    self.compare_view.mode_selector_ui(ui);
                    let batch_label = format!("批量对比 ({selected_count})");
                    let batch_clicked = if selected_count >= 2 {
                        widgets::compact_primary_button(ui, &batch_label, true).clicked()
                    } else {
                        widgets::compact_secondary_button(ui, &batch_label, false).clicked()
                    };
                    if batch_clicked {
                        self.start_batch_compare();
                    }
                });
            }

            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(avail);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                let canvas = self.canvas_size_for_view(ctx);
                if widgets::compact_secondary_button(
                    ui,
                    "适应窗口",
                    has_image || selected_count >= 2,
                )
                .clicked()
                {
                    self.compare_view.fit_to_window(canvas);
                }
                if widgets::compact_secondary_button(
                    ui,
                    "100%",
                    has_image || selected_count >= 2,
                )
                .clicked()
                {
                    self.compare_view.set_zoom_100(canvas);
                }
                if widgets::compact_secondary_button(ui, "撤销标注", has_image).clicked() {
                    if let Some(id) = self.current_image {
                        if let Err(e) = self.service.undo_last_annotation(id) {
                            self.error = Some(e.to_string());
                        } else {
                            self.load_current_annotations();
                            let _ = self.reload_images();
                        }
                    }
                }
                if widgets::compact_secondary_button(
                    ui,
                    "仅显示未评审",
                    self.current_batch.is_some(),
                )
                .clicked()
                {
                    self.sidebar.filter.status = Some(ReviewStatus::Pending);
                    let _ = self.reload_images();
                }
                if widgets::compact_secondary_button(ui, "重置筛选", true).clicked() {
                    self.sidebar.filter.reset_filters();
                    self.sidebar.show_recycle = false;
                    let _ = self.reload_images();
                }
                if ui
                    .checkbox(
                        &mut self.config.auto_advance_on_status,
                        "自动跳下一张未评审",
                    )
                    .changed()
                {
                    let _ = self.config.save();
                }
            });

            ui.add_space(6.0);
            let approved = self
                .current_batch
                .map(|batch_id| {
                    self.batch_stats
                        .iter()
                        .find(|(id, _)| *id == batch_id)
                        .map(|(_, s)| s.approved)
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            let reflux_label = format!("回流转换队列 ({approved})");
            let reflux_clicked = if narrow {
                widgets::full_width_primary_button(ui, &reflux_label, approved > 0).clicked()
            } else {
                widgets::compact_primary_button(ui, &reflux_label, approved > 0).clicked()
            };
            if reflux_clicked {
                self.enqueue_approved_to_convert();
            }
        });
    }

    fn layout_three_column(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        host: &dyn ReviewPanelHost,
        dark: bool,
    ) {
        const GAP: f32 = 10.0;
        let avail = ui.available_width();
        let mut left_w = theme::REVIEW_LEFT_W;
        let mut right_w = theme::REVIEW_RIGHT_W;
        let min_center = theme::REVIEW_CENTER_MIN_W;
        let budget = (avail - GAP * 2.0).max(0.0);
        if budget < left_w + right_w + min_center {
            let side_budget = (budget - min_center).max(400.0);
            let scale = (side_budget / (left_w + right_w)).clamp(0.65, 1.0);
            left_w = (theme::REVIEW_LEFT_W * scale).max(200.0);
            right_w = (theme::REVIEW_RIGHT_W * scale).max(200.0);
        }
        let center_w = (budget - left_w - right_w).max(min_center);
        // 预留底边，避免贴齐窗口底边裁切最后一行控件
        let row_h = (ui.available_height() - 8.0).max(240.0);

        ui.horizontal_top(|ui| {
            // egui 的 add_space 不会抵消 item_spacing；若不置零，列宽之和会超出 avail，右侧被裁切
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, row_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(left_w);
                    ui.set_max_width(left_w);
                    ui.set_width(left_w);
                    ui.set_min_height(row_h);
                    ui.set_max_height(row_h);
                    // 上：批次+筛选可滚；下：图片列表占满剩余高度（避免底部按钮被裁切）
                    self.left_column_split_viewport(ui, ctx, dark, row_h);
                },
            );

            ui.add_space(GAP);

            ui.allocate_ui_with_layout(
                egui::vec2(center_w, row_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(center_w);
                    ui.set_max_width(center_w);
                    ui.set_width(center_w);
                    ui.set_min_height(row_h);
                    ui.set_max_height(row_h);
                    self.center_column(ui, ctx, host);
                },
            );

            ui.add_space(GAP);

            ui.allocate_ui_with_layout(
                egui::vec2(right_w, row_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(right_w);
                    ui.set_max_width(right_w);
                    ui.set_width(right_w);
                    ui.set_min_height(row_h);
                    ui.set_max_height(row_h);
                    egui::ScrollArea::vertical()
                        .id_salt("review_three_col_right")
                        .max_height(row_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let content_w = ui
                                .available_width()
                                .min(ui.max_rect().width())
                                .max(160.0);
                            ui.set_max_width(content_w);
                            ui.set_width(content_w);
                            self.right_column(ctx, ui, dark);
                            ui.add_space(24.0);
                        });
                },
            );
        });
    }

    /// 中等宽度：左列表 | 画布并排（限高可滚），属性全宽沉底，由外层滚动承载。
    fn layout_two_column(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        host: &dyn ReviewPanelHost,
        dark: bool,
    ) {
        const GAP: f32 = 10.0;
        let avail = ui.available_width();
        let left_w = theme::REVIEW_LEFT_W
            .min(avail * 0.38)
            .clamp(200.0, theme::REVIEW_LEFT_W);
        let center_w = (avail - left_w - GAP).max(theme::REVIEW_CENTER_MIN_W);
        // 固定画布区高度，避免按剩余高度百分比挤压「图片」列表
        let row_h = 400.0_f32.min(ui.available_height().max(280.0)).max(280.0);

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, row_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(left_w);
                    ui.set_max_width(left_w);
                    ui.set_width(left_w);
                    egui::ScrollArea::vertical()
                        .id_salt("review_two_col_left")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let content_w = ui
                                .available_width()
                                .min(ui.max_rect().width())
                                .max(120.0);
                            ui.set_width(content_w);
                            self.left_column(ui, ctx, dark);
                        });
                },
            );

            ui.add_space(GAP);

            ui.allocate_ui_with_layout(
                egui::vec2(center_w, row_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(center_w);
                    ui.set_max_width(center_w);
                    ui.set_width(center_w);
                    self.center_column(ui, ctx, host);
                },
            );
        });

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        widgets::grouped_section(ui, "属性与详情", |ui| {
            ui.set_width(ui.available_width());
            self.right_column(ctx, ui, dark);
        });
    }

    fn layout_stacked(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        host: &dyn ReviewPanelHost,
        dark: bool,
    ) {
        ui.set_width(ui.available_width());
        widgets::mode_tab_bar(
            ui,
            &mut self.stack_pane,
            &[
                (ReviewStackPane::List, "列表"),
                (ReviewStackPane::Canvas, "画布"),
                (ReviewStackPane::Inspector, "属性"),
            ],
        );
        ui.add_space(10.0);

        match self.stack_pane {
            ReviewStackPane::List => {
                let col_w = ui.available_width();
                ui.set_width(col_w);
                self.left_column(ui, ctx, dark);
            }
            ReviewStackPane::Canvas => {
                // 整页滚动场景下给画布固定可视高度，避免吃光后续空间
                let area_h = 420.0;
                let area_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(area_w, area_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(area_w);
                        ui.set_min_height(area_h);
                        ui.set_max_height(area_h);
                        self.center_column(ui, ctx, host);
                    },
                );
            }
            ReviewStackPane::Inspector => {
                let col_w = ui.available_width();
                ui.set_width(col_w);
                self.right_column(ctx, ui, dark);
            }
        }
    }

    fn left_column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, dark: bool) {
        let col_w = ui.available_width();
        ui.set_min_width(col_w);
        ui.set_max_width(col_w);
        ui.set_width(col_w);
        self.left_batch_section(ui, dark);
        ui.add_space(12.0);
        widgets::grouped_section(ui, "图片", |ui| {
            let list_action = image_list_ui(
                ui,
                ctx,
                &self.images,
                self.current_image,
                &mut self.sidebar,
                &mut self.list_thumbs,
            );
            self.apply_image_list_action(list_action);
        });
        self.left_batch_stats(ui, dark);
    }

    /// 定高三栏左列：上半批次+筛选可滚，下半列表占满剩余高度。
    fn left_column_split_viewport(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        dark: bool,
        viewport_h: f32,
    ) {
        let col_w = ui.available_width();
        ui.set_width(col_w);
        // 无图时把高度让给上方批次/筛选；有图时上下约各半
        let top_budget = if self.images.is_empty() {
            (viewport_h - 72.0).clamp(200.0, viewport_h.max(200.0))
        } else {
            (viewport_h * 0.50).clamp(168.0, (viewport_h - 140.0).max(168.0))
        };

        egui::ScrollArea::vertical()
            .id_salt("review_left_top")
            .max_height(top_budget)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let content_w = ui
                    .available_width()
                    .min(ui.max_rect().width())
                    .max(120.0);
                ui.set_width(content_w);
                self.left_batch_section(ui, dark);
                ui.add_space(10.0);
                widgets::section_header(ui, "图片");
                ui.add_space(6.0);
                if filter_sort_ui(ui, &mut self.sidebar) {
                    let _ = self.reload_images();
                }
                ui.add_space(8.0);
            });

        ui.add_space(6.0);
        let list_h = (ui.available_height() - 8.0).max(48.0);
        widgets::grouped_section_frame(ui, |ui| {
            let list_w = ui
                .available_width()
                .min(ui.max_rect().width())
                .max(120.0);
            ui.set_width(list_w);
            // 扣除分组框内边距，避免高度溢出裁切
            let body_h = (list_h - 28.0).max(40.0);
            let list_action = image_list_body_ui(
                ui,
                ctx,
                &self.images,
                self.current_image,
                &mut self.sidebar,
                &mut self.list_thumbs,
                Some(body_h),
            );
            self.apply_image_list_action(list_action);
        });
        self.left_batch_stats(ui, dark);
    }

    fn left_batch_section(&mut self, ui: &mut egui::Ui, dark: bool) {
        let _ = dark;
        widgets::grouped_section(ui, "批次", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.sidebar.batch_name_input)
                    .hint_text("批次名称")
                    .desired_width(f32::INFINITY)
                    .margin(egui::vec2(12.0, 10.0)),
            );
            ui.add_space(4.0);
            if let Some(id) =
                batch_list_ui(ui, &self.batches, &self.batch_stats, self.current_batch)
            {
                self.current_batch = Some(id);
                self.current_image = None;
                self.list_thumbs.clear();
                let _ = self.reload_images();
            }
        });
    }

    fn left_batch_stats(&self, ui: &mut egui::Ui, dark: bool) {
        if let Some(batch_id) = self.current_batch {
            if let Some((_, stats)) = self.batch_stats.iter().find(|(id, _)| *id == batch_id) {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format_stats(stats))
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                );
            }
        }
    }

    fn apply_image_list_action(&mut self, list_action: crate::review::ui::sidebar::ImageListAction) {
        if list_action.reload {
            let _ = self.reload_images();
        }
        if let Some(id) = list_action.selected {
            self.select_image(id);
        }
    }

    fn center_column(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        host: &dyn ReviewPanelHost,
    ) {
        let dark = ui.style().visuals.dark_mode;
        let multi_active = self.compare_view.mode == CompareDisplayMode::MultiSplit;
        let section_title = if multi_active {
            format!("画布 · 多图对比 ({})", self.sidebar.selected_ids.len())
        } else if self.compare_view.is_compare_active() {
            format!("画布 · {}对比", self.compare_view.mode_label())
        } else {
            "画布".into()
        };
        widgets::grouped_section(ui, &section_title, |ui| {
            self.canvas_area_size = ui.available_size();
            if multi_active {
                let sources = self.selected_compare_sources();
                if sources.len() < 2 {
                    ui.vertical_centered(|ui| {
                        ui.add_space(32.0);
                        ui.label(
                            RichText::new(
                                "请先在左侧勾选至少 2 张，再点常用栏左侧蓝色「批量对比」",
                            )
                            .color(theme::secondary_label(dark)),
                        );
                    });
                    return;
                }
                if sources.len() > MAX_MULTI_COMPARE_PANES {
                    widgets::error_banner(
                        ui,
                        &format!("最多同时并排对比 {MAX_MULTI_COMPARE_PANES} 张，请减少勾选数量"),
                    );
                    return;
                }
                self.compare_view.ui_multi(ui, ctx, &sources);
                if let Some(name) = sources.first().map(|(_, _, label)| label.as_str()) {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("共 {} 张 · 首张 {}", sources.len(), name))
                            .size(12.0)
                            .color(theme::secondary_label(dark)),
                    );
                }
                return;
            }

            if let Some(item) = self.current_item().cloned() {
                let cache_thumb =
                    crate::review::service::ThumbnailService::valid_cache_path(&item.file_path);
                let item_thumb = item
                    .thumbnail_path
                    .as_ref()
                    .filter(|p| p.exists())
                    .cloned();
                let thumb_owned = item_thumb.or(cache_thumb);
                let thumb_ref = thumb_owned.as_deref();
                let display_path =
                    if crate::review::service::is_non_filesystem_path(&item.file_path) {
                        thumb_ref.unwrap_or(item.file_path.as_path())
                    } else {
                        item.file_path.as_path()
                    };
                self.update_converted_preview(host.output_directory(), &item.file_path);

                let events = {
                    let mut events = self.compare_view.tools_ui(ui);
                    events.extend(self.compare_view.ui(
                        ui,
                        ctx,
                        display_path,
                        self.converted_preview.as_deref(),
                        thumb_ref,
                        &self.current_annotations,
                    ));
                    events
                };
                self.handle_canvas_events(events, item.id);

                ui.add_space(4.0);
                ui.label(
                    RichText::new(item.file_path.display().to_string())
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                );
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(48.0);
                    ui.label(
                        RichText::new("请选择评审批次与图片，或从转换队列导入")
                            .color(theme::secondary_label(dark)),
                    );
                });
            }
        });
    }

    fn tab_review(&mut self, ui: &mut egui::Ui, dark: bool) {
        widgets::section_label(ui, "备注");
        let edit_w = (ui.available_width() - 2.0).max(80.0);
        if ui
            .add(
                egui::TextEdit::multiline(&mut self.remark_buf)
                    .desired_width(edit_w)
                    .margin(egui::vec2(12.0, 10.0))
                    .desired_rows(4),
            )
            .changed()
        {
            if let Some(id) = self.current_image {
                let remark = self.remark_buf.clone();
                if self.data_source == DataSource::Remote {
                    self.set_remote_remark(id, remark);
                } else {
                    if let Err(e) = self.service.set_remark(id, &remark) {
                        self.error = Some(e.to_string());
                    } else if let Some(note) =
                        self.sync_remote_review_item(id, None, Some(remark), None)
                    {
                        self.set_status(note);
                    }
                }
            }
        }

        if let Some(item) = self.current_item() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "标注 {} 条 · {}",
                    self.current_annotations.len(),
                    item.status.label()
                ))
                .size(12.0)
                .color(theme::secondary_label(dark)),
            );
            ui.label(
                RichText::new(format!(
                    "评审时间：{}",
                    item.updated_at.format("%Y-%m-%d %H:%M")
                ))
                .size(12.0)
                .color(theme::secondary_label(dark)),
            );
        }
    }

    fn tab_info(&mut self, ui: &mut egui::Ui) {
        let item = self.current_item().cloned();
        if properties_panel_ui(ui, &mut self.properties, item.as_ref()) {
            if let Some(id) = self.current_image {
                if let Err(e) = self
                    .service
                    .update_convert_params(id, &self.properties.convert_draft)
                {
                    self.error = Some(e.to_string());
                }
            }
        }
    }

    fn tab_analysis(&mut self, ui: &mut egui::Ui, dark: bool) {
        let Some(analysis) = &self.current_analysis else {
            if let Some(err) = &self.analysis_error {
                widgets::error_banner(ui, err);
            } else {
                ui.label(RichText::new("选择图片后显示直方图").color(theme::secondary_label(dark)));
            }
            return;
        };

        widgets::settings_subheading(ui, "亮度");
        histogram_ui(
            ui,
            &analysis.luminance_histogram,
            egui::Color32::from_gray(180),
            96.0,
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("平均亮度：{:.1}", analysis.average_luminance));
            ui.label(format!(
                "暗部裁切：{:.2}%",
                analysis.shadow_clip_ratio * 100.0
            ));
            ui.label(format!(
                "高光裁切：{:.2}%",
                analysis.highlight_clip_ratio * 100.0
            ));
        });

        ui.add_space(12.0);
        widgets::settings_subheading(ui, "RGB");
        histogram_ui(
            ui,
            &analysis.red_histogram,
            egui::Color32::from_rgb(220, 72, 72),
            54.0,
        );
        histogram_ui(
            ui,
            &analysis.green_histogram,
            egui::Color32::from_rgb(80, 190, 110),
            54.0,
        );
        histogram_ui(
            ui,
            &analysis.blue_histogram,
            egui::Color32::from_rgb(90, 130, 230),
            54.0,
        );
        ui.label(
            RichText::new(format!(
                "分析尺寸：{} × {}，直方图使用预览级采样",
                analysis.width, analysis.height
            ))
            .size(12.0)
            .color(theme::secondary_label(dark)),
        );
    }

    fn tab_annotations(&mut self, ui: &mut egui::Ui, dark: bool) {
        if self.current_image.is_none() {
            ui.label(RichText::new("选择图片以查看标注").color(theme::secondary_label(dark)));
            return;
        }
        if self.current_annotations.is_empty() {
            ui.label(RichText::new("暂无标注").color(theme::secondary_label(dark)));
            return;
        }
        let mut clear_all = false;
        ui.horizontal(|ui| {
            ui.label(format!("共 {} 条标注", self.current_annotations.len()));
            if widgets::compact_secondary_button(ui, "清空全部", true).clicked() {
                clear_all = true;
            }
        });
        ui.add_space(6.0);

        let mut delete_id: Option<i64> = None;
        let mut focus_ann: Option<i64> = None;
        let selected_ann = self.compare_view.left_canvas().selected_id();
        egui::ScrollArea::vertical()
            .id_salt("annotation_list_tab")
            .max_height(260.0)
            .show(ui, |ui| {
                for (idx, ann) in self.current_annotations.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let dot = ann.style.color;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            5.0,
                            egui::Color32::from_rgba_unmultiplied(dot[0], dot[1], dot[2], dot[3]),
                        );
                        let row_selected = selected_ann == Some(ann.id);
                        let label = format!("{}. {}", idx + 1, annotation_kind_label(ann.kind));
                        let row_resp = ui.selectable_label(row_selected, label);
                        row_resp.clone().on_hover_text("点击定位到画布");
                        if row_resp.clicked() {
                            focus_ann = Some(ann.id);
                        }
                        if !ann.content.is_empty() {
                            ui.label(
                                RichText::new(truncate_text(&ann.content, 16))
                                    .size(12.0)
                                    .color(theme::secondary_label(dark)),
                            );
                        }
                        if widgets::compact_secondary_button(ui, "删除", true).clicked() {
                            delete_id = Some(ann.id);
                        }
                    });
                }
            });

        if let Some(aid) = focus_ann {
            if let Some(ann) = self.current_annotations.iter().find(|a| a.id == aid) {
                self.compare_view.focus_annotation(ann);
                self.set_status("已定位到标注");
            }
        }

        if clear_all {
            let ids: Vec<i64> = self.current_annotations.iter().map(|a| a.id).collect();
            match self.service.batch_clear_annotations(&ids) {
                Ok(()) => {
                    self.load_current_annotations();
                    let _ = self.reload_images();
                    self.set_status("已清空标注");
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        } else if let Some(aid) = delete_id {
            match self.service.remove_annotation(aid) {
                Ok(()) => {
                    self.load_current_annotations();
                    let _ = self.reload_images();
                    self.set_status("已删除标注");
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn tab_tags(&mut self, ui: &mut egui::Ui, dark: bool) {
        // 新建标签
        ui.horizontal(|ui| {
            let palette = crate::review::ReviewTag::palette();
            let color = palette[self.new_tag_color_idx % palette.len()];
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
            ui.painter().rect_filled(
                rect,
                4.0,
                egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
            );
            if resp.clicked() {
                self.new_tag_color_idx = (self.new_tag_color_idx + 1) % palette.len();
            }
            resp.on_hover_text("点击切换颜色");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_tag_name)
                    .hint_text("新标签名…")
                    .desired_width(120.0),
            );
            if widgets::compact_secondary_button(ui, "添加", !self.new_tag_name.trim().is_empty())
                .clicked()
            {
                let name = self.new_tag_name.trim().to_string();
                match self.service.create_tag(&name, color) {
                    Ok(_) => {
                        self.new_tag_name.clear();
                        self.new_tag_color_idx = (self.new_tag_color_idx + 1) % palette.len();
                        let _ = self.reload_tags();
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        });
        ui.add_space(8.0);

        if self.all_tags.is_empty() {
            ui.label(RichText::new("暂无标签，先添加一个").color(theme::secondary_label(dark)));
            return;
        }

        let has_image = self.current_image.is_some();
        let mut toggles: Vec<(i64, bool)> = Vec::new();
        let mut delete_id: Option<i64> = None;
        let mut rename_commit: Option<(i64, String)> = None;

        egui::ScrollArea::vertical()
            .id_salt("tags_tab")
            .max_height(300.0)
            .show(ui, |ui| {
                let tags = self.all_tags.clone();
                for tag in &tags {
                    ui.horizontal(|ui| {
                        let c = tag.color;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            6.0,
                            egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                        );

                        if let Some((rid, buf)) = self.renaming_tag.as_mut() {
                            if *rid == tag.id {
                                let resp =
                                    ui.add(egui::TextEdit::singleline(buf).desired_width(110.0));
                                if resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    rename_commit = Some((tag.id, buf.clone()));
                                }
                                if widgets::compact_secondary_button(ui, "确定", true).clicked() {
                                    rename_commit = Some((tag.id, buf.clone()));
                                }
                                return;
                            }
                        }

                        let mut on = self.current_image_tags.contains(&tag.id);
                        if ui
                            .add_enabled(has_image, egui::Checkbox::new(&mut on, &tag.name))
                            .changed()
                        {
                            toggles.push((tag.id, on));
                        }
                        if widgets::compact_secondary_button(ui, "改名", true).clicked() {
                            self.renaming_tag = Some((tag.id, tag.name.clone()));
                        }
                        if widgets::compact_secondary_button(ui, "删除", true).clicked() {
                            delete_id = Some(tag.id);
                        }
                    });
                }
            });

        let had_toggles = !toggles.is_empty();
        for (tag_id, on) in toggles {
            if let Some(image_id) = self.current_image {
                if let Err(e) = self.service.set_image_tag(image_id, tag_id, on) {
                    self.error = Some(e.to_string());
                }
            }
        }
        if had_toggles {
            self.reload_current_image_tags();
            if let Some(image_id) = self.current_image {
                let names = self.current_remote_tag_names();
                if let Some(note) = self.sync_remote_review_item(image_id, None, None, Some(names))
                {
                    self.set_status(note);
                }
            }
        }
        if let Some((tag_id, name)) = rename_commit {
            let name = name.trim().to_string();
            if !name.is_empty() {
                let _ = self.service.rename_tag(tag_id, &name);
            }
            self.renaming_tag = None;
            let _ = self.reload_tags();
        }
        if let Some(tag_id) = delete_id {
            if let Err(e) = self.service.delete_tag(tag_id) {
                self.error = Some(e.to_string());
            }
            self.reload_current_image_tags();
            let _ = self.reload_tags();
        }
    }

    fn reload_tags(&mut self) -> ReviewResult<()> {
        self.all_tags = self.service.list_tags()?;
        self.sidebar.available_tags = self.all_tags.clone();
        Ok(())
    }

    fn reload_current_image_tags(&mut self) {
        if self.data_source == DataSource::Remote {
            self.current_image_tags.clear();
            return;
        }
        if let Some(id) = self.current_image {
            self.current_image_tags = self.service.tags_for_image(id).unwrap_or_default();
        } else {
            self.current_image_tags.clear();
        }
    }

    fn right_column(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, dark: bool) {
        let col_w = ui
            .available_width()
            .min(ui.max_rect().width())
            .max(120.0);
        ui.set_max_width(col_w);
        ui.set_width(col_w);

        let tabs = [
            (RightTab::Review, "评审属性"),
            (RightTab::Info, "图片信息"),
            (RightTab::Analysis, "分析"),
            (RightTab::Annotations, "标注列表"),
            (RightTab::Tags, "标签"),
        ];
        widgets::tab_grid_selector(ui, "review_right_tabs", &tabs, self.right_tab, |tab| {
            self.right_tab = tab;
        });
        ui.add_space(10.0);

        match self.right_tab {
            RightTab::Review => self.tab_review(ui, dark),
            RightTab::Info => self.tab_info(ui),
            RightTab::Analysis => self.tab_analysis(ui, dark),
            RightTab::Annotations => self.tab_annotations(ui, dark),
            RightTab::Tags => self.tab_tags(ui, dark),
        }

        ui.add_space(16.0);
        self.batch_ops_ui(ctx, ui, dark);
    }

    fn batch_ops_ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, dark: bool) {
        let section_w = ui
            .available_width()
            .min(ui.max_rect().width())
            .max(100.0);
        ui.set_max_width(section_w);
        ui.set_width(section_w);
        widgets::grouped_section(ui, "批量操作", |ui| {
            let inner_w = ui
                .available_width()
                .min(ui.max_rect().width())
                .max(80.0);
            ui.set_max_width(inner_w);
            ui.set_width(inner_w);
            ui.label(
                RichText::new(format!("已选 {} 张", self.sidebar.selected_ids.len()))
                    .font(theme::section_font())
                    .color(theme::primary_label(dark)),
            );

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.set_max_width(inner_w);
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add_sized(
                    egui::vec2(36.0, widgets::TOOLBAR_ROW_HEIGHT),
                    egui::Label::new(
                        RichText::new("状态")
                            .size(13.0)
                            .color(theme::primary_label(dark)),
                    ),
                );
                let combo_w = ui.available_width().min(ui.max_rect().width()).max(48.0);
                widgets::toolbar_combo_box(
                    ui,
                    "batch_status_target",
                    self.batch_target_status.label(),
                    combo_w,
                    |ui| {
                        for s in [
                            ReviewStatus::Pending,
                            ReviewStatus::Approved,
                            ReviewStatus::NeedsFix,
                            ReviewStatus::Rejected,
                        ] {
                            if ui
                                .selectable_label(self.batch_target_status == s, s.label())
                                .clicked()
                            {
                                self.batch_target_status = s;
                            }
                        }
                    },
                );
            });

            ui.add_space(8.0);
            let gap = 6.0;
            let cell = widgets::equal_cell_width(inner_w, gap, 2);
            ui.horizontal(|ui| {
                ui.set_max_width(inner_w);
                ui.spacing_mut().item_spacing.x = gap;
                if widgets::full_width_secondary_button_in(ui, "更新状态", true, cell).clicked() {
                    self.dialog = Some(DialogState::ConfirmBatchOp(BatchOpKind::SetStatus(
                        self.batch_target_status,
                    )));
                }
                if widgets::full_width_secondary_button_in(ui, "清空标注", true, cell).clicked() {
                    self.dialog = Some(DialogState::ConfirmBatchOp(BatchOpKind::ClearAnnotations));
                }
            });
            ui.add_space(gap);
            if widgets::full_width_secondary_button(ui, "复制当前标注到所选", true).clicked() {
                self.dialog = Some(DialogState::ConfirmBatchOp(
                    BatchOpKind::CopyCurrentAnnotations,
                ));
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_max_width(inner_w);
                ui.spacing_mut().item_spacing.x = gap;
                if widgets::tab_chip_sized(
                    ui,
                    "覆盖备注",
                    cell,
                    self.batch_remark_mode == RemarkWriteMode::Overwrite,
                    true,
                ) {
                    self.batch_remark_mode = RemarkWriteMode::Overwrite;
                }
                if widgets::tab_chip_sized(
                    ui,
                    "追加备注",
                    cell,
                    self.batch_remark_mode == RemarkWriteMode::Append,
                    true,
                ) {
                    self.batch_remark_mode = RemarkWriteMode::Append;
                }
            });
            ui.add_space(gap);
            if widgets::full_width_primary_button(ui, "批量写入备注", true).clicked() {
                self.dialog = Some(DialogState::ConfirmBatchOp(BatchOpKind::AddRemark));
            }

            if !self.last_batch_annotation_ids.is_empty() {
                ui.add_space(gap);
                if widgets::full_width_secondary_button(ui, "撤销上次批量标注", true).clicked() {
                    match self
                        .service
                        .undo_batch_annotations(&self.last_batch_annotation_ids)
                    {
                        Ok(()) => {
                            self.last_batch_annotation_ids.clear();
                            self.load_current_annotations();
                            self.set_status("已撤销上次批量标注");
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(RichText::new("批量截图").strong());
            let screenshot_targets = self.screenshot_target_items().len();
            ui.label(
                RichText::new(format!("目标 {screenshot_targets} 张"))
                    .size(12.0)
                    .color(theme::secondary_label(dark)),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.set_max_width(inner_w);
                ui.spacing_mut().item_spacing.x = gap;
                if widgets::tab_chip_sized(
                    ui,
                    "选中图片",
                    cell,
                    !self.screenshot_use_all_visible,
                    true,
                ) {
                    self.screenshot_use_all_visible = false;
                }
                if widgets::tab_chip_sized(
                    ui,
                    "当前列表",
                    cell,
                    self.screenshot_use_all_visible,
                    true,
                ) {
                    self.screenshot_use_all_visible = true;
                }
            });
            ui.add_space(6.0);
            ui.checkbox(&mut self.screenshot_include_annotations, "包含标注");
            ui.horizontal(|ui| {
                ui.set_max_width(inner_w);
                ui.spacing_mut().item_spacing.x = gap;
                ui.add_sized(
                    egui::vec2(36.0, widgets::TOOLBAR_ROW_HEIGHT),
                    egui::Label::new(
                        RichText::new("格式")
                            .size(13.0)
                            .color(theme::primary_label(dark)),
                    ),
                );
                let format_label = self.screenshot_format.extension().to_uppercase();
                let combo_w = ui.available_width().min(ui.max_rect().width()).max(48.0);
                widgets::toolbar_combo_box(
                    ui,
                    "image_batch_screenshot_format",
                    &format_label,
                    combo_w,
                    |ui| {
                        if ui
                            .selectable_label(
                                self.screenshot_format == ScreenshotFormat::Jpeg,
                                "JPG",
                            )
                            .clicked()
                        {
                            self.screenshot_format = ScreenshotFormat::Jpeg;
                        }
                        if ui
                            .selectable_label(
                                self.screenshot_format == ScreenshotFormat::Png,
                                "PNG",
                            )
                            .clicked()
                        {
                            self.screenshot_format = ScreenshotFormat::Png;
                        }
                    },
                );
            });
            ui.checkbox(&mut self.screenshot_write_json, "同时导出 JSON 清单");
            ui.checkbox(
                &mut self.screenshot_write_contact_sheet,
                "生成索引图（自动分页）",
            );
            ui.label(
                RichText::new("默认生成 CSV；失败项不中断整批")
                    .weak()
                    .size(11.0),
            );
            if self.screenshot_job.is_running() {
                if let Some(progress) = self.screenshot_job.progress() {
                    ui.add(egui::ProgressBar::new(progress.fraction()).show_percentage());
                    if let Some(label) = ProgressReporter::status_label(progress.as_ref()) {
                        ui.label(RichText::new(label).weak().size(11.0));
                    }
                }
            }
            ui.add_space(6.0);
            let export_enabled = screenshot_targets > 0 && !self.screenshot_job.is_running();
            if widgets::full_width_secondary_button(ui, "批量导出截图…", export_enabled).clicked()
            {
                self.export_batch_screenshots(ctx);
            }
        });
    }

    fn screenshot_target_items(&self) -> Vec<&ReviewImageItem> {
        if self.screenshot_use_all_visible {
            self.images.iter().collect()
        } else {
            self.images
                .iter()
                .filter(|item| self.sidebar.selected_ids.contains(&item.id))
                .collect()
        }
    }

    fn export_batch_screenshots(&mut self, ctx: &egui::Context) {
        let targets: Vec<(i64, PathBuf)> = self
            .screenshot_target_items()
            .into_iter()
            .map(|item| (item.id, item.file_path.clone()))
            .collect();
        if targets.is_empty() {
            self.error = Some("请先选择或加载图片".into());
            return;
        }
        if self.screenshot_job.is_running() {
            return;
        }
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            let request = BatchImageScreenshotRequest {
                items: targets.clone(),
                output_dir: dir.clone(),
                include_annotations: self.screenshot_include_annotations,
                format: self.screenshot_format,
                quality: 85,
                naming_template: "{index}_{filename}.{ext}".into(),
                write_csv_manifest: true,
                write_json_manifest: self.screenshot_write_json,
                write_contact_sheet: self.screenshot_write_contact_sheet,
            };
            let total = targets.len();
            self.screenshot_job_started = Some(Instant::now());
            self.screenshot_job_dir = Some(dir);
            self.screenshot_job.spawn(ctx, total, move |progress| {
                let repo = SqliteReviewRepository::open().map_err(|e| e.to_string())?;
                BatchImageScreenshotService::export(&repo, &request, Some(&*progress))
                    .map_err(|e| e.to_string())
            });
        }
    }

    fn poll_screenshot_job(&mut self, ctx: &egui::Context) {
        let Some(result) = self.screenshot_job.poll(ctx) else {
            return;
        };
        let started = self
            .screenshot_job_started
            .take()
            .unwrap_or_else(Instant::now);
        let dir = self
            .screenshot_job_dir
            .take()
            .unwrap_or_else(|| PathBuf::from("."));
        match result {
            Ok(result) => {
                let msg = format!(
                    "已导出截图 {} 张（成功 {} · 失败 {}）→ {}",
                    result.requested,
                    result.succeeded,
                    result.failed,
                    dir.display()
                );
                self.set_status(&msg);
                let status = if result.failed == 0 {
                    ActionHistoryStatus::Succeeded
                } else if result.succeeded > 0 {
                    ActionHistoryStatus::PartiallyFailed
                } else {
                    ActionHistoryStatus::Failed
                };
                self.record_action_history(
                    "批量导出截图",
                    dir.display().to_string(),
                    status,
                    result.succeeded,
                    result.failed,
                    result.requested,
                    started.elapsed().as_millis() as u64,
                    Some(format!(
                        "格式：{} · 标注：{} · JSON {} · 索引 {}",
                        self.screenshot_format.extension().to_uppercase(),
                        self.screenshot_include_annotations,
                        if result.json_manifest.is_some() {
                            "是"
                        } else {
                            "否"
                        },
                        format_contact_sheets(&result.contact_sheets),
                    )),
                );
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }
    fn record_action_history(
        &self,
        operation: impl Into<String>,
        target: impl Into<String>,
        status: ActionHistoryStatus,
        success_count: usize,
        failure_count: usize,
        total_count: usize,
        elapsed_ms: u64,
        detail: Option<String>,
    ) {
        let entry = ActionHistoryEntry {
            finished_at_unix: prefs::now_unix(),
            module: "图片评审".into(),
            operation: operation.into(),
            target: target.into(),
            status,
            success_count,
            failure_count,
            total_count,
            elapsed_ms,
            detail,
        };
        let mut gui_prefs = GuiPrefs::load();
        gui_prefs.push_action_history(entry);
        let _ = gui_prefs.save();
    }

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.clone() else {
            return;
        };

        match dialog {
            DialogState::ConfirmBatchOp(op) => {
                egui::Window::new("确认批量操作")
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(batch_op_description(op));
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, "确认", true).clicked() {
                                self.run_batch_op(op);
                                self.dialog = None;
                            }
                            if widgets::secondary_button(ui, "取消", true).clicked() {
                                self.dialog = None;
                            }
                        });
                    });
            }
            DialogState::IrreversibleStatus {
                target,
                warnings,
                mut confirm,
            } => {
                egui::Window::new("不可逆状态变更")
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "以下 {} 张图片将从「驳回」变更为「{}」，请确认：",
                            warnings.len(),
                            target.label()
                        ));
                        ScrollArea::vertical()
                            .id_salt("review_irreversible_warnings")
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for w in &warnings {
                                    ui.label(RichText::new(&w.message).size(12.0));
                                }
                            });
                        ui.checkbox(&mut confirm, "我已了解该操作不可自动撤销");
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(confirm, |ui| {
                                if widgets::primary_button(ui, "继续执行", confirm).clicked() {
                                    self.apply_batch_status(target, true);
                                    self.dialog = None;
                                }
                            });
                            if widgets::secondary_button(ui, "取消", true).clicked() {
                                self.dialog = None;
                            }
                        });
                    });
                if self.dialog.is_some() {
                    self.dialog = Some(DialogState::IrreversibleStatus {
                        target,
                        warnings,
                        confirm,
                    });
                }
            }
        }
    }

    fn run_batch_op(&mut self, op: BatchOpKind) {
        let ids = self.sidebar.selected_ids.clone();
        if ids.is_empty() {
            self.error = Some("请先在列表中多选图片".into());
            return;
        }

        match op {
            BatchOpKind::SetStatus(status) => self.start_batch_status(ids, status),
            BatchOpKind::ClearAnnotations => match self.service.batch_clear_annotations(&ids) {
                Ok(()) => {
                    let _ = self.reload_images();
                    self.load_current_annotations();
                    self.set_status("已清空所选图片标注");
                }
                Err(e) => self.error = Some(e.to_string()),
            },
            BatchOpKind::AddRemark => {
                if self.data_source == DataSource::Remote {
                    let text = self.remark_buf.clone();
                    let mut changed = 0usize;
                    for id in ids {
                        let current = self
                            .images
                            .iter()
                            .find(|item| item.id == id)
                            .map(|item| item.remark.clone())
                            .unwrap_or_default();
                        let remark = match self.batch_remark_mode {
                            RemarkWriteMode::Overwrite => text.clone(),
                            RemarkWriteMode::Append if current.trim().is_empty() => text.clone(),
                            RemarkWriteMode::Append => format!("{current}\n{text}"),
                        };
                        if self.set_remote_remark(id, remark).is_some() {
                            changed += 1;
                        }
                    }
                    self.set_status(format!("已更新 {changed} 张远程图片备注"));
                    return;
                }
                let result = self.service.batch_add_remarks(&BatchRemarkRequest {
                    image_ids: ids,
                    text: self.remark_buf.clone(),
                    mode: self.batch_remark_mode,
                });
                match result {
                    Ok(r) if r.failures.is_empty() => {
                        let _ = self.reload_images();
                        self.set_status(format!("已更新 {} 张图片备注", r.success_count));
                    }
                    Ok(r) => self.error = Some(r.failures[0].reason.clone()),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            BatchOpKind::CopyCurrentAnnotations => {
                if self.current_annotations.is_empty() {
                    self.error = Some("当前图片无标注可复制".into());
                    return;
                }
                let template_ann = &self.current_annotations[0];
                let tpl = crate::review::storage::AnnotationTemplate {
                    kind: template_ann.kind,
                    position: template_ann.position.clone(),
                    style: template_ann.style.clone(),
                    content: template_ann.content.clone(),
                };
                let result = self.service.batch_add_annotations(&BatchAnnotateRequest {
                    image_ids: ids,
                    template: tpl,
                });
                match result {
                    Ok(r) if r.failures.is_empty() => {
                        self.last_batch_annotation_ids = r.annotation_ids;
                        self.set_status(format!("已批量添加 {} 条标注", r.success_count));
                    }
                    Ok(r) => self.error = Some(r.failures[0].reason.clone()),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }

    fn start_batch_status(&mut self, ids: Vec<i64>, target: ReviewStatus) {
        if self.data_source == DataSource::Remote {
            let warnings: Vec<_> = self
                .images
                .iter()
                .filter(|item| ids.contains(&item.id))
                .filter(|item| is_irreversible_transition(item.status, target))
                .map(|item| StatusTransitionWarning {
                    image_id: item.id,
                    from: item.status,
                    to: target,
                    message: format!(
                        "{}：{} → {}",
                        item.file_path.display(),
                        item.status.label(),
                        target.label()
                    ),
                })
                .collect();
            if warnings.is_empty() {
                self.apply_batch_status(target, true);
            } else {
                self.dialog = Some(DialogState::IrreversibleStatus {
                    target,
                    warnings,
                    confirm: false,
                });
            }
            return;
        }
        let items = match self.service.repo().get_images_by_ids(&ids) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        let warnings: Vec<_> = items
            .iter()
            .filter(|item| is_irreversible_transition(item.status, target))
            .map(|item| StatusTransitionWarning {
                image_id: item.id,
                from: item.status,
                to: target,
                message: format!(
                    "{}：{} → {}",
                    item.file_path.display(),
                    item.status.label(),
                    target.label()
                ),
            })
            .collect();

        if warnings.is_empty() {
            self.apply_batch_status(target, true);
        } else {
            self.dialog = Some(DialogState::IrreversibleStatus {
                target,
                warnings,
                confirm: false,
            });
        }
    }

    fn apply_batch_status(&mut self, target: ReviewStatus, confirm: bool) {
        let ids = self.sidebar.selected_ids.clone();
        if self.data_source == DataSource::Remote {
            let _ = confirm;
            let mut changed = 0usize;
            for id in &ids {
                if self.set_remote_image_status(*id, target).is_some() {
                    changed += 1;
                }
            }
            self.refresh_current_remote_stats();
            self.set_status(format!("已更新 {changed} 张远程图片状态"));
            return;
        }
        let result = self.service.batch_update_status(&BatchStatusRequest {
            image_ids: ids.clone(),
            target_status: target,
            confirm_irreversible: confirm,
        });
        match result {
            Ok(r) if r.applied => {
                let remote_count = self.sync_remote_review_statuses(&ids, target);
                let _ = self.reload_images();
                let _ = self.reload_batches();
                let remote_note = if remote_count > 0 {
                    format!(" · 已同步远程 {remote_count} 张")
                } else {
                    String::new()
                };
                self.set_status(format!(
                    "已更新 {} 张图片状态{remote_note}",
                    r.success_count
                ));
            }
            Ok(r) if !r.warnings.is_empty() => {
                self.dialog = Some(DialogState::IrreversibleStatus {
                    target,
                    warnings: r.warnings,
                    confirm: false,
                });
            }
            Ok(r) if !r.failures.is_empty() => self.error = Some(r.failures[0].reason.clone()),
            Err(e) => self.error = Some(e.to_string()),
            _ => {}
        }
    }

    fn process_pending_import(&mut self) {
        let Some((paths, name)) = self.pending_import.take() else {
            return;
        };
        self.import_from_paths(&paths, &name);
    }

    fn import_from_paths(&mut self, paths: &[PathBuf], batch_name: &str) {
        if paths.is_empty() {
            self.error = Some("转换队列为空".into());
            return;
        }
        match self.service.create_batch_from_paths(batch_name, paths) {
            Ok(id) => {
                self.current_batch = Some(id);
                self.current_image = None;
                self.error = None;
                let _ = self.reload_batches();
                let _ = self.reload_images();
                if let Some(first) = self.images.first() {
                    self.select_image(first.id);
                }
                let remote_note = self.sync_remote_review_batch(batch_name, paths);
                let mut msg = format!(
                    "已从转换队列导入 {} 张图片到批次「{batch_name}」",
                    paths.len()
                );
                if let Some(note) = remote_note {
                    msg.push_str(&format!(" · {note}"));
                }
                self.set_status(msg);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn enqueue_approved_to_convert(&mut self) {
        let Some(batch_id) = self.current_batch else {
            self.error = Some("请先选择评审批次".into());
            return;
        };
        use crate::review::ReviewConversionBridge;
        match self.service.approved_with_params(batch_id) {
            Ok(items) => {
                let n = items.len();
                self.output.enqueue_approved = items.iter().map(|i| i.path.clone()).collect();
                self.output.enqueue_params = items;
                self.output.switch_to_convert = true;
                self.output.status_message =
                    format!("已将 {n} 张「通过」图片加入转换队列，请切换至格式转换页");
                self.status_hint = self.output.status_message.clone();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_hint = msg.into();
    }

    fn switch_to_local(&mut self, reason: impl Into<String>) {
        self.data_source = DataSource::Local;
        self.batches_fetch = None;
        self.items_fetch = None;
        self.asset_fetch = None;
        self.remote_loading = false;
        self.remote_batches.clear();
        self.remote_items.clear();
        self.remote_id_map.clear();
        self.pending_open_remote_batch_id = None;
        self.set_status(reason);
        let _ = self.reload_batches();
        let _ = self.reload_images();
    }

    fn start_remote_batches_fetch(&mut self) {
        if !crate::remote::remote_enabled(&self.remote_config) {
            self.switch_to_local("远程未配置，已回退本地");
            return;
        }
        let cfg = self.remote_config.clone();
        self.remote_loading = true;
        self.batches_fetch = Some(crate::remote::RemoteFetch::spawn(move || {
            let _ = crate::remote::probe_remote_health(&cfg)?;
            crate::remote::list_remote_review_batches(&cfg, crate::remote::RemoteBatchKind::Image)
        }));
    }

    fn start_remote_items_fetch(&mut self, remote_batch_id: String) {
        let cfg = self.remote_config.clone();
        self.remote_batch_id = Some(remote_batch_id.clone());
        self.remote_loading = true;
        self.items_fetch = Some(crate::remote::RemoteFetch::spawn(move || {
            crate::remote::fetch_batch_items_with_thumbs(&cfg, &remote_batch_id)
        }));
    }

    fn poll_remote_fetches(&mut self, ctx: &egui::Context) {
        let batches_result = self.batches_fetch.as_ref().and_then(|fetch| fetch.poll());
        if let Some(result) = batches_result {
            self.batches_fetch = None;
            self.remote_loading = self.items_fetch.is_some() || self.asset_fetch.is_some();
            match result {
                Ok(summaries) => {
                    self.apply_remote_batches(summaries);
                    self.set_status(format!("已加载 {} 个远程评审批次", self.batches.len()));
                }
                Err(e) => self.switch_to_local(format!("远程批次加载失败，已回退本地：{e}")),
            }
            ctx.request_repaint();
        } else if self.batches_fetch.is_some() {
            ctx.request_repaint();
        }
        let items_result = self.items_fetch.as_ref().and_then(|fetch| fetch.poll());
        if let Some(result) = items_result {
            self.items_fetch = None;
            self.remote_loading = self.batches_fetch.is_some() || self.asset_fetch.is_some();
            match result {
                Ok(pairs) => {
                    self.apply_remote_items(pairs);
                    self.set_status(format!("远程条目 {} 张", self.images.len()));
                }
                Err(e) => self.switch_to_local(format!("远程条目加载失败，已回退本地：{e}")),
            }
            ctx.request_repaint();
        } else if self.items_fetch.is_some() {
            ctx.request_repaint();
        }
        let asset_result = self.asset_fetch.as_ref().and_then(|fetch| fetch.poll());
        if let Some(result) = asset_result {
            self.asset_fetch = None;
            self.remote_loading = self.batches_fetch.is_some() || self.items_fetch.is_some();
            match result {
                Ok((image_id, path)) => {
                    if let Some(img) = self.images.iter_mut().find(|i| i.id == image_id) {
                        img.file_path = path;
                    }
                    if self.current_image == Some(image_id) {
                        self.converted_preview = None;
                        self.load_current_analysis();
                        if let Some(item) = self.current_item().cloned() {
                            self.properties.sync_item(&item, None);
                        }
                    }
                    self.set_status("原图已下载到本地缓存");
                }
                Err(e) => self.set_status(format!("原图下载失败：{e}")),
            }
            ctx.request_repaint();
        } else if self.asset_fetch.is_some() {
            ctx.request_repaint();
        }
    }

    fn apply_remote_batches(&mut self, summaries: Vec<crate::remote::RemoteReviewBatchSummary>) {
        self.remote_id_map.clear();
        self.remote_batches = summaries;
        self.batches = self
            .remote_batches
            .iter()
            .map(|s| crate::remote::batch_from_summary(&mut self.remote_id_map, s))
            .collect();
        self.batch_stats = self
            .batches
            .iter()
            .map(|b| (b.id, BatchStats::default()))
            .collect();

        let previous_batch = self.current_batch;
        let preferred = self
            .pending_open_remote_batch_id
            .take()
            .and_then(|rid| self.remote_id_map.local_of(&rid));
        if let Some(local) = preferred {
            self.current_batch = Some(local);
        } else if let Some(cur) = self.current_batch {
            if !self.batches.iter().any(|b| b.id == cur) {
                self.current_batch = self.batches.first().map(|b| b.id);
            }
        } else {
            self.current_batch = self.batches.first().map(|b| b.id);
        }

        if self.current_batch != previous_batch {
            self.current_image = None;
            self.images.clear();
            self.remote_items.clear();
            self.list_thumbs.clear();
        }

        if let Some(bid) = self.current_batch {
            if let Some(rid) = self.remote_id_map.remote_of(bid).map(|s| s.to_string()) {
                self.remote_batch_id = Some(rid.clone());
                self.start_remote_items_fetch(rid);
            }
        } else {
            self.remote_batch_id = None;
            self.images.clear();
            self.remote_items.clear();
        }
    }

    fn apply_remote_items(
        &mut self,
        pairs: Vec<(crate::remote::RemoteReviewItem, Option<PathBuf>)>,
    ) {
        let Some(batch_local) = self.current_batch else {
            return;
        };
        if let Some(rid) = self.remote_id_map.remote_of(batch_local) {
            self.remote_batch_id = Some(rid.to_string());
        }

        self.remote_items = pairs.iter().map(|(i, _)| i.clone()).collect();
        self.refresh_current_remote_stats();
        self.images = pairs
            .iter()
            .map(|(item, thumb)| {
                let file = thumb
                    .clone()
                    .unwrap_or_else(|| crate::remote::placeholder_path_for_asset(&item.asset));
                let thumb_path = thumb.clone();
                let mut img = crate::remote::image_from_remote_item(
                    &mut self.remote_id_map,
                    batch_local,
                    item,
                    file,
                    thumb_path.clone(),
                );
                if let Some(t) = &thumb_path {
                    img.file_path = t.clone();
                }
                img
            })
            .collect();

        self.remote_item_ids.clear();
        for item in &self.remote_items {
            if let Some(local) = self.remote_id_map.local_of(&item.item_id) {
                self.remote_item_ids.insert(local, item.item_id.clone());
            }
        }
        self.sidebar
            .selected_ids
            .retain(|id| self.images.iter().any(|image| image.id == *id));
        if let Some(cur) = self.current_image {
            if !self.images.iter().any(|i| i.id == cur) {
                self.current_image = self.images.first().map(|i| i.id);
            }
        } else {
            self.current_image = self.images.first().map(|i| i.id);
        }
        if let Some(id) = self.current_image {
            self.select_image(id);
        } else {
            self.current_annotations.clear();
            self.current_image_tags.clear();
            self.remark_buf.clear();
        }
    }

    fn ensure_remote_original_downloaded(&mut self, image_id: i64) {
        if self.data_source != DataSource::Remote {
            return;
        }
        if self
            .asset_fetch
            .as_ref()
            .map(|f| f.is_pending())
            .unwrap_or(false)
        {
            return;
        }
        let Some(remote_item_id) = self.remote_item_ids.get(&image_id).cloned() else {
            return;
        };
        let Some(remote_item) = self
            .remote_items
            .iter()
            .find(|i| i.item_id == remote_item_id)
            .cloned()
        else {
            return;
        };
        if let Some(img) = self.images.iter().find(|i| i.id == image_id) {
            let p = &img.file_path;
            let is_thumb = img.thumbnail_path.as_ref() == Some(p);
            if p.exists() && !p.to_string_lossy().starts_with("remote://") && !is_thumb {
                return;
            }
        }
        let cfg = self.remote_config.clone();
        let asset = remote_item.asset.clone();
        self.remote_loading = true;
        self.asset_fetch = Some(crate::remote::RemoteFetch::spawn(move || {
            let path = crate::remote::ensure_remote_asset_local(&cfg, &asset)?;
            Ok((image_id, path))
        }));
    }

    fn refresh_current_remote_stats(&mut self) {
        let Some(batch_local) = self.current_batch else {
            return;
        };
        if let Some((_, stats)) = self
            .batch_stats
            .iter_mut()
            .find(|(id, _)| *id == batch_local)
        {
            *stats = crate::remote::stats_from_items(&self.remote_items);
        }
    }

    fn sync_remote_review_batch(&mut self, name: &str, paths: &[PathBuf]) -> Option<String> {
        if !crate::remote::remote_enabled(&self.remote_config) {
            self.remote_batch_id = None;
            self.remote_item_ids.clear();
            return None;
        }
        if paths.is_empty() {
            return None;
        }

        match crate::remote::create_remote_batch_from_paths(
            &self.remote_config,
            name,
            crate::remote::RemoteBatchKind::Image,
            paths,
        ) {
            Ok(batch) => {
                self.remote_batch_id = Some(batch.batch_id.clone());
                let items = match crate::remote::list_remote_review_items(
                    &self.remote_config,
                    &batch.batch_id,
                ) {
                    Ok(items) => items,
                    Err(e) => {
                        return Some(format!("远程批次 {}（读取条目失败：{e}）", batch.batch_id));
                    }
                };
                self.remember_remote_items(&items);
                let inputs = items.iter().map(|item| item.asset.clone()).collect();
                let extras = vec![
                    ("batch_id".into(), batch.batch_id.clone()),
                    ("batch_name".into(), name.to_string()),
                    ("generate_thumbnails".into(), "true".into()),
                    ("generate_previews".into(), "true".into()),
                ];
                match crate::remote::submit_module_job(
                    &self.remote_config,
                    crate::remote::RemoteJobSource::Review,
                    inputs,
                    extras,
                ) {
                    Ok((status, _)) => Some(format!(
                        "远程批次 {} · 任务 {}",
                        batch.batch_id, status.job_id
                    )),
                    Err(e) => Some(format!("远程批次 {}（任务提交失败：{e}）", batch.batch_id)),
                }
            }
            Err(e) => {
                self.remote_batch_id = None;
                self.remote_item_ids.clear();
                Some(format!("远程批次创建失败：{e}"))
            }
        }
    }

    fn remember_remote_items(&mut self, items: &[crate::remote::RemoteReviewItem]) {
        self.remote_item_ids.clear();
        let mut used = std::collections::HashSet::new();
        for image in &self.images {
            let Some(local_name) = image.file_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Some(item) = items
                .iter()
                .find(|item| !used.contains(&item.item_id) && item.asset.name == local_name)
            {
                used.insert(item.item_id.clone());
                self.remote_item_ids.insert(image.id, item.item_id.clone());
            }
        }
    }

    fn sync_remote_review_statuses(&mut self, ids: &[i64], status: ReviewStatus) -> usize {
        ids.iter()
            .filter(|id| {
                self.sync_remote_review_item(
                    **id,
                    Some(review_status_to_remote(status)),
                    None,
                    None,
                )
                .as_deref()
                    == Some("已同步远程")
            })
            .count()
    }

    fn sync_remote_review_item(
        &mut self,
        local_id: i64,
        status: Option<crate::remote::RemoteReviewItemStatus>,
        remark: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Option<String> {
        if !crate::remote::remote_enabled(&self.remote_config) || self.remote_batch_id.is_none() {
            return None;
        }
        let item_id = self.remote_item_ids.get(&local_id)?.clone();
        match crate::remote::sync_review_item(&self.remote_config, &item_id, status, remark, tags) {
            Ok(_) => Some("已同步远程".into()),
            Err(e) => Some(format!("远程同步失败：{e}")),
        }
    }

    fn set_remote_image_status(&mut self, id: i64, status: ReviewStatus) -> Option<String> {
        if let Some(image) = self.images.iter_mut().find(|image| image.id == id) {
            image.status = status;
            image.updated_at = chrono::Utc::now();
        }
        if let Some(remote_item_id) = self.remote_item_ids.get(&id).cloned() {
            if let Some(item) = self
                .remote_items
                .iter_mut()
                .find(|item| item.item_id == remote_item_id)
            {
                item.status = crate::remote::local_status_to_remote(status);
                item.updated_at = chrono::Utc::now().timestamp().max(0) as u64;
            }
        }
        self.sync_remote_review_item(
            id,
            Some(crate::remote::local_status_to_remote(status)),
            None,
            None,
        )
    }

    fn set_remote_remark(&mut self, id: i64, remark: String) -> Option<String> {
        if let Some(image) = self.images.iter_mut().find(|image| image.id == id) {
            image.remark = remark.clone();
            image.updated_at = chrono::Utc::now();
        }
        if let Some(remote_item_id) = self.remote_item_ids.get(&id).cloned() {
            if let Some(item) = self
                .remote_items
                .iter_mut()
                .find(|item| item.item_id == remote_item_id)
            {
                item.remark = remark.clone();
                item.updated_at = chrono::Utc::now().timestamp().max(0) as u64;
            }
        }
        self.sync_remote_review_item(id, None, Some(remark), None)
    }

    fn current_remote_tag_names(&self) -> Vec<String> {
        self.current_image_tags
            .iter()
            .filter_map(|id| {
                self.all_tags
                    .iter()
                    .find(|tag| tag.id == *id)
                    .map(|tag| tag.name.clone())
            })
            .collect()
    }

    fn sync_current_remote_annotations(&mut self, image_item_id: i64) {
        if !crate::remote::remote_enabled(&self.remote_config)
            || !self.remote_item_ids.contains_key(&image_item_id)
        {
            return;
        }
        for ann in self.current_annotations.clone() {
            self.sync_remote_annotation(&ann);
        }
    }

    fn sync_remote_annotation(&mut self, ann: &Annotation) {
        if !crate::remote::remote_enabled(&self.remote_config) {
            return;
        }
        let Some(item_id) = self.remote_item_ids.get(&ann.image_item_id).cloned() else {
            return;
        };
        let remote = crate::remote::RemoteAnnotation {
            schema_version: crate::remote::REMOTE_SCHEMA_VERSION,
            annotation_id: format!("local-{}", ann.id),
            item_id,
            kind: annotation_kind_to_remote(ann.kind),
            content: ann.content.clone(),
            geometry: annotation_geometry(&ann.position),
            created_at: ann.created_at.timestamp().max(0) as u64,
            locked: ann.locked,
        };
        let _ = crate::remote::save_annotation(&self.remote_config, remote);
    }

    fn handle_shortcut_actions(&mut self, ctx: &egui::Context) {
        let action = handle_shortcuts(&self.service.shortcuts, ctx);
        let Some(action) = action else { return };
        match action {
            ShortcutAction::PrevImage => self.select_relative(-1),
            ShortcutAction::NextImage => self.select_relative(1),
            ShortcutAction::StatusPending => self.set_current_status(ReviewStatus::Pending),
            ShortcutAction::StatusApproved => self.set_current_status(ReviewStatus::Approved),
            ShortcutAction::StatusNeedsFix => self.set_current_status(ReviewStatus::NeedsFix),
            ShortcutAction::StatusRejected => self.set_current_status(ReviewStatus::Rejected),
            ShortcutAction::FitWindow => {
                self.compare_view
                    .fit_to_window(self.canvas_size_for_view(ctx));
            }
            ShortcutAction::ActualSize => {
                self.compare_view
                    .set_zoom_100(self.canvas_size_for_view(ctx));
            }
            ShortcutAction::UndoAnnotation => {
                if let Some(id) = self.current_image {
                    if let Err(e) = self.service.undo_last_annotation(id) {
                        self.error = Some(e.to_string());
                    }
                    self.load_current_annotations();
                }
            }
        }
    }

    fn current_item(&self) -> Option<&ReviewImageItem> {
        let id = self.current_image?;
        self.images.iter().find(|i| i.id == id)
    }

    fn current_index(&self) -> Option<usize> {
        let id = self.current_image?;
        self.images.iter().position(|i| i.id == id)
    }

    fn select_image(&mut self, id: i64) {
        self.current_image = Some(id);
        // 小窗堆叠布局：选中图片后切到画布，避免还要手动点「画布」
        if self.stack_pane == ReviewStackPane::List {
            self.stack_pane = ReviewStackPane::Canvas;
        }
        self.ensure_remote_original_downloaded(id);
        if let Some(item) = self.images.iter().find(|i| i.id == id) {
            self.remark_buf = item.remark.clone();
            self.properties.sync_item(item, None);
            if let Ok(meta) = self
                .service
                .refresh_metadata_cache(item.id, &item.file_path)
            {
                self.properties.sync_item(item, Some(meta));
            }
        }
        self.load_current_analysis();
        if self.data_source == DataSource::Local {
            if let (Some(batch), Some(image)) = (self.current_batch, self.current_image) {
                let _ = self.service.save_session(batch, image);
            }
        }
        self.load_current_annotations();
        self.reload_current_image_tags();
        if let (Some(idx), Some(_batch_id)) = (self.current_index(), self.current_batch) {
            let paths: Vec<_> = self.images.iter().map(|i| i.file_path.clone()).collect();
            let thumbs: Vec<_> = self
                .images
                .iter()
                .map(|i| i.thumbnail_path.clone())
                .collect();
            self.compare_view.prefetch_neighbors(
                &paths,
                idx,
                self.config.prefetch_neighbors,
                &thumbs,
            );
        }
    }

    fn load_current_analysis(&mut self) {
        self.current_analysis = None;
        self.analysis_error = None;
        let Some(path) = self.current_item().map(|item| item.file_path.clone()) else {
            return;
        };
        let key = file_mtime_key(&path);
        if let Some((cached_key, analysis)) = self.analysis_cache.get(&path) {
            if *cached_key == key {
                self.current_analysis = Some(analysis.clone());
                return;
            }
        }
        match ImageAnalysisService::analyze(&path) {
            Ok(analysis) => {
                self.analysis_cache.insert(path, (key, analysis.clone()));
                self.current_analysis = Some(analysis);
            }
            Err(e) => {
                self.analysis_error = Some(format!("图片分析失败：{e}"));
            }
        }
    }

    fn select_relative(&mut self, delta: isize) {
        let Some(current) = self.current_image else {
            return;
        };
        let idx = self.images.iter().position(|i| i.id == current);
        let Some(idx) = idx else { return };
        let new_idx = (idx as isize + delta).clamp(0, self.images.len() as isize - 1) as usize;
        if let Some(item) = self.images.get(new_idx) {
            self.select_image(item.id);
        }
    }

    fn set_current_status(&mut self, status: ReviewStatus) {
        if let Some(id) = self.current_image {
            self.set_image_status(id, status);
        }
    }

    fn set_image_status(&mut self, id: i64, status: ReviewStatus) {
        if self.data_source == DataSource::Remote {
            let remote_note = self
                .set_remote_image_status(id, status)
                .map(|note| format!(" · {note}"))
                .unwrap_or_default();
            self.refresh_current_remote_stats();
            self.set_status(format!("已设为「{}」{remote_note}", status.label()));
            if self.config.auto_advance_on_status {
                match next_image_id(&self.images, id, self.config.auto_advance_target) {
                    Some(next) => self.select_image(next),
                    None => self.set_status("已完成全部评审"),
                }
            }
            return;
        }
        match self.service.set_status(id, status) {
            Ok(()) => {
                let remote_note = self
                    .sync_remote_review_item(id, Some(review_status_to_remote(status)), None, None)
                    .map(|note| format!(" · {note}"))
                    .unwrap_or_default();
                let _ = self.reload_images();
                let _ = self.reload_batches();
                self.set_status(format!("已设为「{}」{remote_note}", status.label()));
                if self.config.auto_advance_on_status {
                    match next_image_id(&self.images, id, self.config.auto_advance_target) {
                        Some(next) => self.select_image(next),
                        None => self.set_status("已完成全部评审"),
                    }
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn load_current_annotations(&mut self) {
        let Some(id) = self.current_image else { return };
        if self.images.iter().all(|i| i.id != id) {
            return;
        }
        if self.data_source == DataSource::Remote {
            self.current_annotations.clear();
            let Some(remote_item_id) = self.remote_item_ids.get(&id).cloned() else {
                return;
            };
            match crate::remote::list_remote_annotations(&self.remote_config, &remote_item_id) {
                Ok(remote_annotations) => {
                    let total = remote_annotations.len();
                    self.current_annotations = remote_annotations
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, ann)| remote_annotation_to_local(id, idx, ann))
                        .collect();
                    if self.current_annotations.len() < total {
                        self.set_status("部分远程标注类型暂不支持显示");
                    }
                }
                Err(e) => self.set_status(format!("远程标注加载失败：{e}")),
            }
            return;
        }
        match self.service.load_annotations(id) {
            Ok(anns) => self.current_annotations = anns,
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn handle_canvas_events(&mut self, events: Vec<AnnotationCanvasEvent>, image_item_id: i64) {
        if self.data_source == DataSource::Remote {
            self.handle_remote_canvas_events(events, image_item_id);
            return;
        }
        let mut changed = false;
        for event in &events {
            match event {
                AnnotationCanvasEvent::CreateAnnotation {
                    kind,
                    position,
                    style,
                    content,
                } => {
                    let ann = Annotation::new_draft(
                        image_item_id,
                        *kind,
                        position.clone(),
                        style.clone(),
                        content.clone(),
                    );
                    if let Err(e) = self.service.add_annotation(&ann).map(|new_id| {
                        let mut saved = ann.clone();
                        saved.id = new_id;
                        self.sync_remote_annotation(&saved);
                    }) {
                        self.error = Some(e.to_string());
                    } else {
                        changed = true;
                    }
                }
                AnnotationCanvasEvent::UpdateAnnotation { id, position } => {
                    if let Err(e) = self.service.update_annotation_position(*id, position) {
                        self.error = Some(e.to_string());
                    } else {
                        changed = true;
                    }
                }
                AnnotationCanvasEvent::UpdateAnnotationContent { id, content } => {
                    if let Err(e) = self.service.update_annotation_content(*id, content) {
                        self.error = Some(e.to_string());
                    } else {
                        changed = true;
                    }
                }
                AnnotationCanvasEvent::DeleteAnnotation { id } => {
                    if let Err(e) = self.service.remove_annotation(*id) {
                        self.error = Some(e.to_string());
                    } else {
                        changed = true;
                    }
                }
                AnnotationCanvasEvent::SelectionChanged { .. }
                | AnnotationCanvasEvent::ToolChanged { .. } => {}
            }
        }
        if changed {
            self.load_current_annotations();
            self.sync_current_remote_annotations(image_item_id);
        }
    }

    fn handle_remote_canvas_events(
        &mut self,
        events: Vec<AnnotationCanvasEvent>,
        image_item_id: i64,
    ) {
        let mut changed = false;
        for event in events {
            match event {
                AnnotationCanvasEvent::CreateAnnotation {
                    kind,
                    position,
                    style,
                    content,
                } => {
                    let mut ann =
                        Annotation::new_draft(image_item_id, kind, position, style, content);
                    ann.id = self.next_remote_annotation_id();
                    self.current_annotations.push(ann.clone());
                    self.sync_remote_annotation(&ann);
                    changed = true;
                }
                AnnotationCanvasEvent::UpdateAnnotation { id, position } => {
                    if let Some(ann) = self.current_annotations.iter_mut().find(|ann| ann.id == id)
                    {
                        ann.position = position;
                        let ann = ann.clone();
                        self.sync_remote_annotation(&ann);
                        changed = true;
                    }
                }
                AnnotationCanvasEvent::UpdateAnnotationContent { id, content } => {
                    if let Some(ann) = self.current_annotations.iter_mut().find(|ann| ann.id == id)
                    {
                        ann.content = content;
                        let ann = ann.clone();
                        self.sync_remote_annotation(&ann);
                        changed = true;
                    }
                }
                AnnotationCanvasEvent::DeleteAnnotation { id } => {
                    let before = self.current_annotations.len();
                    self.current_annotations.retain(|ann| ann.id != id);
                    if self.current_annotations.len() != before {
                        self.set_status("远程标注删除仅更新当前视图");
                        changed = true;
                    }
                }
                AnnotationCanvasEvent::SelectionChanged { .. }
                | AnnotationCanvasEvent::ToolChanged { .. } => {}
            }
        }
        if changed {
            if let Some(image) = self
                .images
                .iter_mut()
                .find(|image| image.id == image_item_id)
            {
                image.annotation_count = self.current_annotations.len() as i32;
            }
        }
    }

    fn next_remote_annotation_id(&self) -> i64 {
        self.current_annotations
            .iter()
            .map(|ann| ann.id)
            .min()
            .unwrap_or(0)
            .min(0)
            - 1
    }

    fn reload_batches(&mut self) -> ReviewResult<()> {
        if self.data_source == DataSource::Remote {
            self.start_remote_batches_fetch();
            return Ok(());
        }
        self.batches = self.service.batch_service().list_batches()?;
        self.batch_stats = self
            .batches
            .iter()
            .map(|b| {
                let stats = self.service.batch_service().batch_stats(b.id)?;
                Ok((b.id, stats))
            })
            .collect::<ReviewResult<Vec<_>>>()?;
        Ok(())
    }

    fn reload_images(&mut self) -> ReviewResult<()> {
        if self.data_source == DataSource::Remote {
            let Some(batch_id) = self.current_batch else {
                self.images.clear();
                self.remote_items.clear();
                return Ok(());
            };
            if let Some(remote_batch_id) = self.remote_id_map.remote_of(batch_id) {
                self.start_remote_items_fetch(remote_batch_id.to_string());
            }
            return Ok(());
        }
        let Some(batch_id) = self.current_batch else {
            self.images.clear();
            return Ok(());
        };
        if self.sidebar.show_recycle {
            self.images = self.service.list_deleted_images(batch_id)?;
        } else {
            self.images = self.service.list_images(batch_id, &self.sidebar.filter)?;
            // 标签维度筛选（需图片-标签映射，故在此叠加）
            if !self.sidebar.filter.tag_ids.is_empty() {
                let ids: Vec<i64> = self.images.iter().map(|i| i.id).collect();
                let map = self.service.tags_for_images(&ids).unwrap_or_default();
                let filter = self.sidebar.filter.clone();
                filter.retain_by_tags(&mut self.images, &map);
            }
        }
        // 缓存当前批次全部图片的标签用于列表色点
        let ids: Vec<i64> = self.images.iter().map(|i| i.id).collect();
        self.sidebar.image_tags = self.service.tags_for_images(&ids).unwrap_or_default();
        let visible: std::collections::HashSet<i64> = self.images.iter().map(|i| i.id).collect();
        self.sidebar.selected_ids.retain(|id| visible.contains(id));
        Ok(())
    }

    fn maybe_scheduled_backup(&mut self) {
        if self.config.backup_interval_minutes == 0 {
            return;
        }
        let minute = chrono::Utc::now().timestamp() as u64 / 60;
        if minute.saturating_sub(self.last_backup_minute)
            >= self.config.backup_interval_minutes as u64
        {
            if crate::review::storage::create_backup().is_ok() {
                self.last_backup_minute = minute;
            }
        }
    }

    fn create_batch_from_folder(&mut self, folder: &Path) {
        let name = self.sidebar.batch_name_input.clone();
        match self.service.create_batch_from_folder(&name, folder, true) {
            Ok(id) => {
                self.current_batch = Some(id);
                self.error = None;
                let _ = self.reload_batches();
                let _ = self.reload_images();
                let remote_paths = review_image_paths(folder, true).unwrap_or_default();
                let remote_note = self.sync_remote_review_batch(&name, &remote_paths);
                let mut msg = format!("已创建批次：{name}");
                if let Some(note) = remote_note {
                    msg.push_str(&format!(" · {note}"));
                }
                self.set_status(msg);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn export_csv(&mut self) {
        let Some(batch_id) = self.current_batch else {
            self.error = Some("请先选择批次".into());
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("review_export.csv")
            .save_file()
        {
            match self.service.export_csv(batch_id, &path) {
                Ok(()) => self.set_status(format!("已导出 CSV：{}", path.display())),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn export_sidecar(&mut self) {
        let Some(item) = self.current_item().cloned() else {
            self.error = Some("请先选择图片".into());
            return;
        };
        match self.service.export_sidecar(item.id, &item.file_path) {
            Ok(p) => self.set_status(format!("已导出标注：{}", p.display())),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn export_batch_json(&mut self) {
        let Some(batch_id) = self.current_batch else {
            self.error = Some("请先选择批次".into());
            return;
        };
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            use crate::review::service::BatchJsonExportRequest;
            match ExportService::export_batch_annotation_json(
                self.service.repo(),
                &BatchJsonExportRequest {
                    batch_id,
                    output_dir: dir.clone(),
                },
            ) {
                Ok(paths) => self.set_status(format!(
                    "已导出 {} 个 JSON 到 {}",
                    paths.len(),
                    dir.display()
                )),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn update_converted_preview(&mut self, output_dir: &str, source: &Path) {
        let out_root = Path::new(output_dir);
        if !out_root.exists() {
            self.converted_preview = None;
            return;
        }
        let file_name = source.file_name();
        let Some(name) = file_name else {
            self.converted_preview = None;
            return;
        };
        let candidate = out_root.join(name);
        self.converted_preview = candidate.exists().then_some(candidate);
    }

    /// 查询路径评审状态（供转换列表展示标签）。
    pub fn status_for_path(&self, path: &Path) -> Option<ReviewStatus> {
        self.service.status_for_path(path).ok().flatten()
    }
}

impl ReviewPanel {
    /// 按列表顺序返回已勾选图片（path, thumb, 显示名）。
    fn selected_compare_sources(&self) -> Vec<(PathBuf, Option<PathBuf>, String)> {
        self.images
            .iter()
            .filter(|item| self.sidebar.selected_ids.contains(&item.id))
            .map(|item| {
                let label = item
                    .file_path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| item.file_path.display().to_string());
                let cache_thumb =
                    crate::review::service::ThumbnailService::valid_cache_path(&item.file_path);
                let thumb = item
                    .thumbnail_path
                    .as_ref()
                    .filter(|p| p.exists())
                    .cloned()
                    .or(cache_thumb);
                let path = if crate::review::service::is_non_filesystem_path(&item.file_path) {
                    thumb
                        .clone()
                        .unwrap_or_else(|| item.file_path.clone())
                } else {
                    item.file_path.clone()
                };
                (path, thumb, label)
            })
            .collect()
    }

    fn start_batch_compare(&mut self) {
        let count = self.sidebar.selected_ids.len();
        if count < 2 {
            self.error = Some("请先在列表勾选至少 2 张图片".into());
            return;
        }
        if count > MAX_MULTI_COMPARE_PANES {
            self.error = Some(format!(
                "最多同时并排对比 {MAX_MULTI_COMPARE_PANES} 张，请减少选择"
            ));
            return;
        }
        let sources = self.selected_compare_sources();
        self.compare_view.prefetch_multi_thumbs(
            &sources
                .iter()
                .map(|(path, thumb, _)| (path.clone(), thumb.clone()))
                .collect::<Vec<_>>(),
        );
        self.compare_view.mode = CompareDisplayMode::MultiSplit;
        self.error = None;
        self.set_status(format!("已进入多图对比（{count} 张）"));
    }

    fn canvas_size_for_view(&self, ctx: &egui::Context) -> egui::Vec2 {
        if self.canvas_area_size.x > 8.0 && self.canvas_area_size.y > 8.0 {
            self.canvas_area_size
        } else {
            viewport_size(ctx)
        }
    }
}

fn review_status_to_remote(status: ReviewStatus) -> crate::remote::RemoteReviewItemStatus {
    match status {
        ReviewStatus::Pending => crate::remote::RemoteReviewItemStatus::Pending,
        ReviewStatus::Approved => crate::remote::RemoteReviewItemStatus::Approved,
        ReviewStatus::NeedsFix => crate::remote::RemoteReviewItemStatus::NeedsFix,
        ReviewStatus::Rejected => crate::remote::RemoteReviewItemStatus::Rejected,
    }
}

fn annotation_kind_to_remote(kind: AnnotationKind) -> crate::remote::RemoteAnnotationKind {
    match kind {
        AnnotationKind::Rectangle => crate::remote::RemoteAnnotationKind::Rectangle,
        AnnotationKind::Arrow => crate::remote::RemoteAnnotationKind::Arrow,
        AnnotationKind::Text => crate::remote::RemoteAnnotationKind::Text,
    }
}

fn annotation_geometry(position: &AnnotationPosition) -> Vec<(String, f64)> {
    match position {
        AnnotationPosition::Rectangle(r) => vec![
            ("x0".into(), r.x0 as f64),
            ("y0".into(), r.y0 as f64),
            ("x1".into(), r.x1 as f64),
            ("y1".into(), r.y1 as f64),
        ],
        AnnotationPosition::Arrow(a) => vec![
            ("x0".into(), a.x0 as f64),
            ("y0".into(), a.y0 as f64),
            ("x1".into(), a.x1 as f64),
            ("y1".into(), a.y1 as f64),
        ],
        AnnotationPosition::Text(t) => vec![("x".into(), t.x as f64), ("y".into(), t.y as f64)],
    }
}

fn remote_annotation_to_local(
    image_item_id: i64,
    idx: usize,
    ann: &crate::remote::RemoteAnnotation,
) -> Option<Annotation> {
    let kind = match ann.kind {
        crate::remote::RemoteAnnotationKind::Rectangle => AnnotationKind::Rectangle,
        crate::remote::RemoteAnnotationKind::Arrow => AnnotationKind::Arrow,
        crate::remote::RemoteAnnotationKind::Text => AnnotationKind::Text,
        crate::remote::RemoteAnnotationKind::Marker
        | crate::remote::RemoteAnnotationKind::Segment => {
            return None;
        }
    };
    let position = remote_annotation_position(kind, &ann.geometry)?;
    let created_at = chrono::DateTime::<chrono::Utc>::from_timestamp(
        ann.created_at.min(i64::MAX as u64) as i64,
        0,
    )
    .unwrap_or_else(chrono::Utc::now);
    Some(Annotation {
        id: -((idx as i64) + 1),
        image_item_id,
        kind,
        position,
        style: AnnotationStyle::default(),
        content: ann.content.clone(),
        created_at,
        locked: ann.locked,
        z_index: idx as i32,
    })
}

fn remote_annotation_position(
    kind: AnnotationKind,
    geometry: &[(String, f64)],
) -> Option<AnnotationPosition> {
    let value = |key: &str| {
        geometry
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value as f32)
    };
    match kind {
        AnnotationKind::Rectangle => Some(AnnotationPosition::Rectangle(RectanglePosition {
            x0: value("x0")?,
            y0: value("y0")?,
            x1: value("x1")?,
            y1: value("y1")?,
        })),
        AnnotationKind::Arrow => Some(AnnotationPosition::Arrow(ArrowPosition {
            x0: value("x0")?,
            y0: value("y0")?,
            x1: value("x1")?,
            y1: value("y1")?,
        })),
        AnnotationKind::Text => Some(AnnotationPosition::Text(TextPosition {
            x: value("x")?,
            y: value("y")?,
        })),
    }
}

fn review_image_paths(folder: &Path, recursive: bool) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let extensions = ["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif", "gif"];
    if recursive {
        for entry in jwalk::WalkDir::new(folder)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                push_review_image_path(&entry.path(), &extensions, &mut out);
            }
        }
    } else {
        for entry in std::fs::read_dir(folder)? {
            let path = entry?.path();
            if path.is_file() {
                push_review_image_path(&path, &extensions, &mut out);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn push_review_image_path(path: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| extensions.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
    {
        out.push(path.to_path_buf());
    }
}
