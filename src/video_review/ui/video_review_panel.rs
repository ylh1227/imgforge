//! 视频评审主面板。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::TimeZone;
use eframe::egui::{
    self, Color32, Context, CornerRadius, Frame, Margin, RichText, ScrollArea, TextureHandle,
};

use crate::gui::prefs::{self, ActionHistoryEntry, ActionHistoryStatus, ExportTemplate, GuiPrefs};
use crate::gui::{theme, widgets, BackgroundJob};
use crate::remote::{
    DataSource, RemoteBatchKind, RemoteFetch, RemoteIdMap, RemoteReviewBatchSummary,
    RemoteReviewItem,
};
use crate::review::domain::image_item::ReviewStatus;
use crate::review::ui::status_buttons;
use crate::ui::progress::ProgressReporter;
use crate::video_review::domain::{
    MarkerKind, VideoBatch, VideoDefect, VideoItem, VideoMarker, VideoSegment, VideoTag,
};
use crate::video_review::service::ffmpeg_backend::FfmpegBackend;
use crate::video_review::service::frame_cache::FrameCache;
use crate::video_review::service::screenshot_service::plan_shots;
use crate::video_review::service::{
    compute_layout, compute_quality_cell_size, default_defect_output_dir, grid_dimensions,
    max_export_duration_ms, offset_after_frame_step, AlignBatchResult, AlignPairResult,
    BatchOperationResult, BatchScreenshotRequest, BatchScreenshotResult, BatchScreenshotService,
    CreateDefectRequest, CreateDefectResult, GridVideoCaptionMode, GridVideoExportQuality,
    ImportFolderOptions, ImportFolderResult, ScreenshotFormat, ScreenshotMode,
    VideoAnalysisService, VideoExportRequest, VideoExportSchema, VideoExportService,
    VideoReviewService, ALIGN_CONFIDENCE_WARN, DEFAULT_DEFECT_HALF_WINDOW_MS,
    DEFAULT_INTERVAL_SECS, DEFAULT_MAX_SHOTS,
};
use crate::video_review::ui::hover_preview::HoverPreviewController;
use crate::video_review::ui::multi_compare::{format_ms, MultiVideoCompare, MAX_COMPARE_VIDEOS};
use crate::video_review::ui::video_list::{
    video_list_body_ui, video_list_toolbar_ui, VideoListAction, VideoListState,
};

#[derive(Debug, Clone, Default)]
pub struct VideoReviewPanelOutput {
    pub status_message: String,
}

#[derive(Debug, Clone)]
struct VideoJiraResultLine {
    text: String,
    browse_url: Option<String>,
}

#[derive(Debug, Clone)]
enum VideoJiraDialog {
    Confirm {
        force_recreate: bool,
        attach: bool,
    },
    Result {
        summary: String,
        lines: Vec<VideoJiraResultLine>,
    },
}

pub struct VideoReviewPanel {
    service: VideoReviewService,
    batches: Vec<VideoBatch>,
    videos: Vec<VideoItem>,
    current_batch: Option<i64>,
    current_video: Option<i64>,
    compare: MultiVideoCompare,
    selected_ids: Vec<i64>,
    video_list_state: VideoListState,
    hover_preview: HoverPreviewController,
    video_tag_map: HashMap<i64, Vec<i64>>,
    remark_buf: String,
    offset_buf: String,
    device_model_buf: String,
    all_tags: Vec<VideoTag>,
    current_tag_ids: Vec<i64>,
    markers: Vec<VideoMarker>,
    segments: Vec<VideoSegment>,
    timeline_thumbs: Vec<(u64, Option<PathBuf>)>,
    thumb_textures: HashMap<String, TextureHandle>,
    new_marker_text: String,
    segment_start_ms: u64,
    segment_end_ms: u64,
    segment_text: String,
    new_tag_name: String,
    new_tag_color_idx: usize,
    right_tab: RightTab,
    output: VideoReviewPanelOutput,
    error: Option<String>,
    status_hint: String,
    compare_mode: bool,
    export_success: Option<String>,
    export_clip_secs: f32,
    export_lossless: bool,
    export_caption_mode: GridVideoCaptionMode,
    screenshot_mode: ScreenshotMode,
    screenshot_use_filtered: bool,
    screenshot_interval_secs: f32,
    screenshot_max_shots: u32,
    screenshot_format: ScreenshotFormat,
    screenshot_write_json: bool,
    screenshot_write_contact_sheet: bool,
    screenshot_job: BackgroundJob<BatchScreenshotResult>,
    screenshot_job_started: Option<Instant>,
    screenshot_job_dir: Option<PathBuf>,
    import_job: BackgroundJob<ImportFolderResult>,
    import_job_started: Option<Instant>,
    import_job_folder: Option<PathBuf>,
    align_job: BackgroundJob<AlignBatchResult>,
    align_review: Option<AlignBatchResult>,
    align_review_open: bool,
    align_prev_offsets: HashMap<i64, i64>,
    defect_job: BackgroundJob<CreateDefectResult>,
    defect_dialog_open: bool,
    defect_title: String,
    defect_description: String,
    defect_severity: u8,
    defect_half_secs: f32,
    defect_include_grid: bool,
    defect_include_clip: bool,
    defect_include_frames: bool,
    defect_include_sources: bool,
    defect_lossless: bool,
    defect_mark_issue: bool,
    defect_set_needs_fix: bool,
    defect_output_dir: Option<PathBuf>,
    defect_align_method: String,
    defect_require_align: bool,
    defects: Vec<VideoDefect>,
    selected_defect_ids: Vec<i64>,
    jira_config: crate::jira::JiraConfig,
    jira_job: BackgroundJob<crate::jira::JiraBatchSubmitResult>,
    jira_dialog: Option<VideoJiraDialog>,
    action_history: Vec<ActionHistoryEntry>,
    video_export_column_keys: Vec<String>,
    video_export_columns_initialized: bool,
    video_export_template_name: String,
    batch_remark_buf: String,
    batch_tag_ids: Vec<i64>,
    pending_delete_marker: Option<i64>,
    pending_delete_segment: Option<i64>,
    remote_config: crate::remote::RemoteConfig,
    remote_batch_id: Option<String>,
    remote_item_ids: HashMap<i64, String>,
    data_source: DataSource,
    remote_batches: Vec<RemoteReviewBatchSummary>,
    remote_items: Vec<RemoteReviewItem>,
    remote_id_map: RemoteIdMap,
    batches_fetch: Option<RemoteFetch<Vec<RemoteReviewBatchSummary>>>,
    items_fetch: Option<RemoteFetch<Vec<(RemoteReviewItem, Option<PathBuf>)>>>,
    asset_fetch: Option<RemoteFetch<(i64, PathBuf)>>,
    remote_loading: bool,
    pending_open_remote_batch_id: Option<String>,
    /// 小窗堆叠布局当前分段。
    stack_pane: VideoStackPane,
}

const MARKER_TEMPLATES: &[&str] = &["画面抖动", "字幕错误", "音画不同步", "黑场", "曝光异常"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RightTab {
    #[default]
    Review,
    Info,
    Markers,
    Tags,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum VideoStackPane {
    #[default]
    List,
    Player,
}

impl VideoReviewPanel {
    pub fn new() -> Result<Self, String> {
        let service = VideoReviewService::open().map_err(|e| e.to_string())?;
        let mut remote_config = crate::remote::RemoteConfig::default();
        remote_config.apply_env_overrides();
        let mut jira_config = crate::jira::JiraConfig::default();
        jira_config.apply_env_overrides();
        let data_source =
            DataSource::from_remote_enabled(crate::remote::remote_enabled(&remote_config));
        let mut panel = Self {
            service,
            batches: Vec::new(),
            videos: Vec::new(),
            current_batch: None,
            current_video: None,
            compare: MultiVideoCompare::default(),
            selected_ids: Vec::new(),
            video_list_state: VideoListState::default(),
            hover_preview: HoverPreviewController::default(),
            video_tag_map: HashMap::new(),
            remark_buf: String::new(),
            offset_buf: String::new(),
            device_model_buf: String::new(),
            all_tags: Vec::new(),
            current_tag_ids: Vec::new(),
            markers: Vec::new(),
            segments: Vec::new(),
            timeline_thumbs: Vec::new(),
            thumb_textures: HashMap::new(),
            new_marker_text: String::new(),
            segment_start_ms: 0,
            segment_end_ms: 0,
            segment_text: String::new(),
            new_tag_name: String::new(),
            new_tag_color_idx: 0,
            right_tab: RightTab::default(),
            output: VideoReviewPanelOutput::default(),
            error: None,
            status_hint: String::new(),
            compare_mode: false,
            export_success: None,
            export_clip_secs: 10.0,
            export_lossless: false,
            export_caption_mode: GridVideoCaptionMode::default(),
            screenshot_mode: ScreenshotMode::CurrentTime,
            screenshot_use_filtered: false,
            screenshot_interval_secs: DEFAULT_INTERVAL_SECS as f32,
            screenshot_max_shots: DEFAULT_MAX_SHOTS as u32,
            screenshot_format: ScreenshotFormat::Jpeg,
            screenshot_write_json: false,
            screenshot_write_contact_sheet: false,
            screenshot_job: BackgroundJob::default(),
            screenshot_job_started: None,
            screenshot_job_dir: None,
            import_job: BackgroundJob::default(),
            import_job_started: None,
            import_job_folder: None,
            align_job: BackgroundJob::default(),
            align_review: None,
            align_review_open: false,
            align_prev_offsets: HashMap::new(),
            defect_job: BackgroundJob::default(),
            defect_dialog_open: false,
            defect_title: String::new(),
            defect_description: String::new(),
            defect_severity: 2,
            defect_half_secs: (DEFAULT_DEFECT_HALF_WINDOW_MS as f32) / 1000.0,
            defect_include_grid: true,
            defect_include_clip: true,
            defect_include_frames: true,
            defect_include_sources: true,
            defect_lossless: true,
            defect_mark_issue: true,
            defect_set_needs_fix: true,
            defect_output_dir: None,
            defect_align_method: "manual".into(),
            defect_require_align: false,
            defects: Vec::new(),
            selected_defect_ids: Vec::new(),
            jira_config,
            jira_job: BackgroundJob::default(),
            jira_dialog: None,
            action_history: GuiPrefs::load().action_history,
            video_export_column_keys: Vec::new(),
            video_export_columns_initialized: false,
            video_export_template_name: String::from("默认导出"),
            batch_remark_buf: String::new(),
            batch_tag_ids: Vec::new(),
            pending_delete_marker: None,
            pending_delete_segment: None,
            remote_config,
            remote_batch_id: None,
            remote_item_ids: HashMap::new(),
            data_source,
            remote_batches: Vec::new(),
            remote_items: Vec::new(),
            remote_id_map: RemoteIdMap::new(),
            batches_fetch: None,
            items_fetch: None,
            asset_fetch: None,
            remote_loading: false,
            pending_open_remote_batch_id: None,
            stack_pane: VideoStackPane::default(),
        };
        panel.reload_batches().map_err(|e| e.to_string())?;
        Ok(panel)
    }

    pub fn set_remote_config(&mut self, remote_config: crate::remote::RemoteConfig) {
        let changed = self.remote_config != remote_config;
        self.remote_config = remote_config;
        let want =
            DataSource::from_remote_enabled(crate::remote::remote_enabled(&self.remote_config));
        if self.data_source != want {
            self.data_source = want;
            let _ = self.reload_batches();
        } else if changed && self.data_source == DataSource::Remote {
            let _ = self.reload_batches();
        }
    }

    pub fn set_jira_config(&mut self, mut jira_config: crate::jira::JiraConfig) {
        jira_config.apply_env_overrides();
        self.jira_config = jira_config;
    }

    pub fn take_output(&mut self) -> VideoReviewPanelOutput {
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

    pub fn ui(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        self.poll_remote_fetches(ctx);
        self.poll_errors();
        self.poll_screenshot_job(ctx);
        self.poll_import_job(ctx);
        self.poll_align_job(ctx);
        self.poll_defect_job(ctx);
        self.poll_jira_job(ctx);
        self.show_jira_dialogs(ctx);
        self.draw_align_review(ctx);
        self.draw_defect_dialog(ctx);

        widgets::navigation_header(ui, "标记片段、对比与批量抽帧");
        widgets::page_header_gap(ui);
        self.show_ffmpeg_banner(ui);

        let geo = widgets::SideMainGeometry::compute(
            ui.available_size(),
            theme::VIDEO_REVIEW_WIDE_BREAKPOINT,
            theme::VIDEO_REVIEW_LEFT_W,
        );

        match geo.mode {
            widgets::SideMainMode::SideBySide => {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(geo.left_w, geo.row_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_width(geo.left_w);
                            ui.set_max_width(geo.left_w);
                            ui.set_width(geo.left_w);
                            ScrollArea::vertical()
                                .id_salt("video_review_left")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let content_w = ui
                                        .available_width()
                                        .min(ui.max_rect().width())
                                        .max(120.0);
                                    ui.set_width(content_w);
                                    self.left_sidebar_ui(ctx, ui);
                                });
                        },
                    );
                    ui.add_space(geo.gap);
                    ui.allocate_ui_with_layout(
                        egui::vec2(geo.main_w, geo.row_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_width(geo.main_w);
                            ui.set_max_width(geo.main_w);
                            ui.set_width(geo.main_w);
                            self.center_ui(
                                ctx,
                                ui,
                                egui::vec2(geo.main_w, (geo.row_h - 8.0).max(200.0)),
                            );
                        },
                    );
                    ui.allocate_exact_size(
                        egui::vec2(geo.right_inset, geo.row_h),
                        egui::Sense::hover(),
                    );
                });
            }
            widgets::SideMainMode::Stacked => {
                widgets::mode_tab_bar(
                    ui,
                    &mut self.stack_pane,
                    &[
                        (VideoStackPane::List, "列表"),
                        (VideoStackPane::Player, "播放"),
                    ],
                );
                ui.add_space(10.0);
                match self.stack_pane {
                    VideoStackPane::List => {
                        let col_w = ui.available_width();
                        ui.set_width(col_w);
                        self.left_sidebar_ui(ctx, ui);
                    }
                    VideoStackPane::Player => {
                        let area =
                            egui::vec2(ui.available_width(), ui.available_height().max(360.0));
                        ui.set_min_height(area.y);
                        self.center_ui(ctx, ui, area);
                    }
                }
            }
        }
    }

    fn show_ffmpeg_banner(&self, ui: &mut egui::Ui) {
        let avail = self.service.availability();
        if avail.ffmpeg_ok && avail.ffprobe_ok {
            return;
        }
        widgets::warning_banner(
            ui,
            "ffmpeg/ffprobe 未检测到，请安装并加入 PATH 后重启。视频导入与抽帧功能不可用。",
        );
        ui.add_space(4.0);
    }

    fn left_sidebar_ui(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let col_w = ui.available_width();
        ui.set_min_width(col_w);
        ui.set_max_width(col_w);
        ui.set_width(col_w);

        widgets::grouped_section(ui, "批次", |ui| {
            if crate::remote::remote_enabled(&self.remote_config) {
                ui.horizontal_wrapped(|ui| {
                    ui.label("数据源");
                    if ui
                        .selectable_label(self.data_source == DataSource::Remote, "远程")
                        .clicked()
                    {
                        if self.data_source != DataSource::Remote {
                            self.data_source = DataSource::Remote;
                            self.start_remote_batches_fetch();
                        }
                    }
                    if ui
                        .selectable_label(self.data_source == DataSource::Local, "本地")
                        .clicked()
                    {
                        if self.data_source != DataSource::Local {
                            self.switch_to_local("已切换到本地数据源");
                        }
                    }
                    if self.remote_loading {
                        ui.spinner();
                        ui.label("加载中…");
                    }
                });
                ui.add_space(6.0);
                if widgets::full_width_secondary_button(
                    ui,
                    "刷新远程",
                    self.data_source == DataSource::Remote && !self.remote_loading,
                )
                .clicked()
                {
                    self.start_remote_batches_fetch();
                }
                ui.add_space(6.0);
            }
            if widgets::full_width_primary_button(
                ui,
                if self.import_job.is_running() {
                    "导入中…"
                } else {
                    "导入视频文件夹…"
                },
                !self.import_job.is_running(),
            )
            .clicked()
            {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.start_import_folder(ctx, folder);
                }
            }
            if self.import_job.is_running() {
                if let Some(progress) = self.import_job.progress() {
                    ui.add(egui::ProgressBar::new(progress.fraction()).show_percentage());
                    if let Some(label) = ProgressReporter::status_label(progress.as_ref()) {
                        ui.label(RichText::new(label).weak().size(11.0));
                    }
                }
            }
            ui.add_space(6.0);
            ScrollArea::vertical()
                .id_salt("video_review_batch_list")
                .max_height(120.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if self.batches.is_empty() {
                        ui.label(
                            RichText::new("暂无批次，先导入视频文件夹")
                                .size(12.0)
                                .weak(),
                        );
                    }
                    for batch in &self.batches.clone() {
                        let selected = self.current_batch == Some(batch.id);
                        if ui
                            .add_sized(
                                egui::vec2(ui.available_width(), 22.0),
                                egui::SelectableLabel::new(selected, &batch.name),
                            )
                            .clicked()
                        {
                            self.current_batch = Some(batch.id);
                            let _ = self.reload_videos();
                        }
                    }
                });
            if let Some(batch_id) = self.current_batch {
                ui.add_space(4.0);
                if self.data_source == DataSource::Remote {
                    let stats = self.remote_video_stats();
                    ui.label(
                        RichText::new(format!(
                            "待评审 {} · 通过 {} · 待修正 {} · 驳回 {}",
                            stats.pending, stats.approved, stats.needs_fix, stats.rejected
                        ))
                        .small()
                        .weak(),
                    );
                } else if let Ok(stats) = self.service.batch_stats(batch_id) {
                    ui.label(
                        RichText::new(format!(
                            "待评审 {} · 通过 {} · 待修正 {} · 驳回 {}",
                            stats.pending, stats.approved, stats.needs_fix, stats.rejected
                        ))
                        .small()
                        .weak(),
                    );
                }
            }
        });

        ui.add_space(8.0);
        widgets::grouped_section(ui, "视频列表", |ui| {
            let mut action = VideoListAction::default();
            video_list_toolbar_ui(
                ui,
                &mut self.video_list_state,
                self.selected_ids.len(),
                &mut action,
            );
            self.apply_video_list_action(&mut action);

            let mode = self.video_list_state.mode;
            let videos = self.videos.clone();
            let current_video = self.current_video;
            let selected_ids = self.selected_ids.clone();
            let all_tags = self.all_tags.clone();
            let video_tag_map = self.video_tag_map.clone();
            let current_time_ms = self.compare.current_time_ms;
            video_list_body_ui(
                ctx,
                ui,
                &mode,
                &videos,
                current_video,
                &selected_ids,
                &all_tags,
                &video_tag_map,
                current_time_ms,
                &self.service,
                &mut self.hover_preview,
                &mut self.thumb_textures,
                &mut action,
            );
            self.apply_video_list_action(&mut action);
        });

        ui.add_space(8.0);
        self.recent_tasks_ui(ui);
    }

    fn apply_video_list_action(&mut self, action: &mut VideoListAction) {
        if action.reload_videos {
            let _ = self.reload_videos();
            action.reload_videos = false;
        }
        if let Some(id) = action.select_video.take() {
            self.select_video(id);
        }
        if action.enter_compare {
            if self.selected_ids.len() >= 2 {
                self.compare_mode = true;
                self.compare.set_compare_ids(self.selected_ids.clone());
            }
            action.enter_compare = false;
        }
        if action.clear_selection {
            self.selected_ids.clear();
            action.clear_selection = false;
        }
        if let Some((id, on)) = action.toggle_compare_id.take() {
            if on {
                if self.selected_ids.len() < MAX_COMPARE_VIDEOS && !self.selected_ids.contains(&id)
                {
                    self.selected_ids.push(id);
                }
            } else {
                self.selected_ids.retain(|x| *x != id);
            }
        }
    }

    fn center_ui(&mut self, ctx: &Context, ui: &mut egui::Ui, area: egui::Vec2) {
        self.attribute_panel_ui(ctx, ui, area.x);
        ui.add_space(8.0);

        fixed_grouped_section(
            ui,
            if self.compare_mode {
                "多视频对比"
            } else {
                "时间轴"
            },
            area.x,
            |ui| {
                if self.compare_mode {
                    let ffmpeg_ok = self.service.availability().ffmpeg_ok;
                    ui.horizontal(|ui| {
                        if widgets::compact_secondary_button(ui, "← 单视频", true).clicked() {
                            self.compare_mode = false;
                        }
                        ui.label(format!(
                            "同步时间：{}",
                            format_ms(self.compare.current_time_ms)
                        ));
                        let can_export = self.selected_ids.len() >= 2;
                        let busy = self.align_job.is_running() || self.defect_job.is_running();
                        if widgets::compact_secondary_button(
                            ui,
                            if self.align_job.is_running() {
                                "对齐中…"
                            } else {
                                "帧对齐"
                            },
                            can_export && ffmpeg_ok && !busy,
                        )
                        .clicked()
                        {
                            self.start_frame_align(ctx);
                        }
                        if !self.align_prev_offsets.is_empty()
                            && widgets::compact_secondary_button(ui, "撤销对齐", !busy).clicked()
                        {
                            self.undo_align_offsets();
                        }
                        if self.align_review.is_some()
                            && widgets::compact_secondary_button(ui, "对齐结果", true).clicked()
                        {
                            self.align_review_open = true;
                        }
                        if widgets::compact_primary_button(ui, "导出宫格", can_export).clicked() {
                            self.export_contact_sheet();
                        }
                        if widgets::compact_secondary_button(
                            ui,
                            "导出视频",
                            can_export && ffmpeg_ok,
                        )
                        .clicked()
                        {
                            self.export_compare_grid_video();
                        }
                        if widgets::compact_primary_button(
                            ui,
                            "从对比新建缺陷",
                            can_export && ffmpeg_ok && !busy,
                        )
                        .clicked()
                        {
                            self.open_defect_dialog();
                        }
                    });
                }

                let max_dur = self
                    .current_video_item()
                    .map(|v| v.duration_ms)
                    .unwrap_or(0)
                    .max(1);

                let mut t = self.compare.current_time_ms.min(max_dur) as f64;
                if ui
                    .add(
                        egui::Slider::new(&mut t, 0.0..=max_dur as f64)
                            .smart_aim(true)
                            .text("时间"),
                    )
                    .changed()
                {
                    self.compare.current_time_ms = t as u64;
                }

                if !self.compare_mode {
                    if let Some(video) = self.current_video_item().cloned() {
                        self.draw_timeline_strip(ctx, ui, &video);
                    }
                }

                ui.add_space(6.0);
                let view_h = ui.available_height().max(120.0);
                let pane_w = ui.available_width();
                if self.compare_mode && self.selected_ids.len() >= 2 {
                    let action = self.compare.ui(
                        ctx,
                        ui,
                        &self.service,
                        &self.videos,
                        egui::vec2(pane_w, view_h.max(120.0)),
                    );
                    self.apply_compare_nudges(&action.frame_nudges);
                } else if let Some(video) = self.current_video_item().cloned() {
                    let mut compare = MultiVideoCompare::with_time(self.compare.current_time_ms);
                    let action = compare.ui(
                        ctx,
                        ui,
                        &self.service,
                        std::slice::from_ref(&video),
                        egui::vec2(pane_w, view_h.max(120.0)),
                    );
                    self.compare.current_time_ms = compare.current_time_ms;
                    self.apply_compare_nudges(&action.frame_nudges);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("选择或导入视频开始评审");
                    });
                }
            },
        );
    }

    fn draw_timeline_strip(&mut self, ctx: &Context, ui: &mut egui::Ui, video: &VideoItem) {
        if self.timeline_thumbs.is_empty() {
            if let Ok(thumbs) = self.service.timeline_thumbs(video, 10) {
                self.timeline_thumbs = thumbs;
            }
        }
        let thumbs = self.timeline_thumbs.clone();
        ui.horizontal(|ui| {
            for (t, path) in &thumbs {
                let selected = (*t).abs_diff(self.compare.current_time_ms) < 500;
                ui.vertical(|ui| {
                    if let Some(p) = path {
                        if let Some(tex) = self.load_thumb(ctx, p) {
                            let resp = ui.add(
                                egui::ImageButton::new((tex.id(), egui::vec2(56.0, 32.0)))
                                    .selected(selected),
                            );
                            if resp.clicked() {
                                self.compare.current_time_ms = *t;
                            }
                        }
                    }
                    ui.label(RichText::new(format_ms(*t)).size(9.0).weak());
                });
            }
        });
    }

    fn attribute_panel_ui(&mut self, ctx: &Context, ui: &mut egui::Ui, width: f32) {
        let tabs = [
            (RightTab::Review, "评审"),
            (RightTab::Info, "信息"),
            (RightTab::Markers, "标记"),
            (RightTab::Tags, "标签"),
            (RightTab::Export, "导出"),
        ];

        // 标题与「时间轴」左对齐；Tab 横排与「评审」芯片左缘对齐，放在圆角框外避免裁切。
        widgets::section_header(ui, "属性");
        ui.add_space(6.0);
        let mut picked = self.right_tab;
        widgets::tab_selector_row(ui, "video_attr_tab", &tabs, self.right_tab, |tab| {
            picked = tab;
        });
        self.right_tab = picked;
        ui.add_space(8.0);

        const ATTR_PANEL_MAX_H: f32 = 220.0;
        fixed_grouped_frame(ui, width, |ui| {
            ScrollArea::vertical()
                .id_salt("video_review_attr_panel")
                .max_height(ATTR_PANEL_MAX_H)
                .show(ui, |ui| match self.right_tab {
                    RightTab::Review => self.review_tab_ui(ui),
                    RightTab::Info => self.info_tab_ui(ui),
                    RightTab::Markers => self.markers_tab_ui(ui),
                    RightTab::Tags => self.tags_tab_ui(ui),
                    RightTab::Export => self.export_tab_ui(ctx, ui),
                });
        });
    }

    fn review_tab_ui(&mut self, ui: &mut egui::Ui) {
        let Some(video) = self.current_video_item().cloned() else {
            ui.label("未选择视频");
            return;
        };
        let mut picked = None;
        if let Some(status) = status_buttons(ui, Some(video.status)) {
            picked = Some(status);
        }
        if let Some(s) = picked {
            if self.data_source == DataSource::Remote {
                if let Some(note) = self.set_remote_video_status(video.id, s) {
                    self.status_hint = note;
                }
            } else if self.service.update_status(video.id, s).is_ok() {
                let _ = self.sync_remote_video_item(
                    video.id,
                    Some(crate::remote::local_status_to_remote(s)),
                    None,
                    None,
                );
                let _ = self.reload_videos();
            }
        }
        ui.add_space(6.0);
        ui.label("备注");
        if ui.text_edit_multiline(&mut self.remark_buf).lost_focus() {
            if self.data_source == DataSource::Remote {
                if let Some(note) = self.set_remote_video_remark(video.id, self.remark_buf.clone())
                {
                    self.status_hint = note;
                }
            } else if self
                .service
                .update_remark(video.id, &self.remark_buf)
                .is_ok()
            {
                let _ = self.sync_remote_video_item(
                    video.id,
                    None,
                    Some(self.remark_buf.clone()),
                    None,
                );
            }
        }
        ui.add_space(6.0);
        ui.label("偏移校准 (ms)");
        ui.horizontal(|ui| {
            if ui.text_edit_singleline(&mut self.offset_buf).lost_focus() {
                if let Ok(v) = self.offset_buf.parse::<i64>() {
                    if self.data_source == DataSource::Remote {
                        if let Some(item) = self.videos.iter_mut().find(|item| item.id == video.id)
                        {
                            item.offset_ms = v;
                            item.updated_at = chrono::Utc::now();
                        }
                    } else {
                        let _ = self.service.update_offset(video.id, v);
                        let _ = self.reload_videos();
                    }
                }
            }
        });

        if !self.selected_ids.is_empty() {
            ui.add_space(10.0);
            ui.separator();
            ui.label(RichText::new(format!("批量操作（{} 个）", self.selected_ids.len())).strong());
            let mut picked = None;
            if let Some(status) = status_buttons(ui, None) {
                picked = Some(status);
            }
            if let Some(s) = picked {
                let ids = self.selected_ids.clone();
                let started = Instant::now();
                if self.data_source == DataSource::Remote {
                    let remote_count = self.sync_remote_video_statuses(&ids, s);
                    self.record_action(
                        "批量更新状态",
                        format!("{} 个远程视频 → {}", ids.len(), s.label()),
                        ActionHistoryStatus::Succeeded,
                        remote_count,
                        0,
                        ids.len(),
                        started.elapsed().as_millis() as u64,
                        None,
                    );
                    self.status_hint = format!("已批量更新 {remote_count} 个远程视频状态");
                    return;
                }
                let result = self.service.batch_update_status_result(&ids, s);
                self.record_batch_action(
                    "批量更新状态",
                    format!("{} 个视频 → {}", ids.len(), s.label()),
                    &result,
                    started.elapsed().as_millis() as u64,
                );
                if result.is_success() {
                    let remote_count = self.sync_remote_video_statuses(&ids, s);
                    let _ = self.reload_videos();
                    self.status_hint = format!("已批量更新 {} 个视频状态", result.applied);
                    if remote_count > 0 {
                        self.status_hint
                            .push_str(&format!(" · 已同步远程 {remote_count} 个"));
                    }
                } else {
                    self.error = Some(result.failures.join("\n"));
                }
            }
            ui.label("批量备注追加");
            ui.text_edit_multiline(&mut self.batch_remark_buf);
            if widgets::compact_secondary_button(
                ui,
                "追加到选中",
                !self.batch_remark_buf.trim().is_empty(),
            )
            .clicked()
            {
                let ids = self.selected_ids.clone();
                let text = self.batch_remark_buf.clone();
                let started = Instant::now();
                if self.data_source == DataSource::Remote {
                    let mut changed = 0usize;
                    for id in &ids {
                        let current = self
                            .videos
                            .iter()
                            .find(|item| item.id == *id)
                            .and_then(|item| item.remark.clone())
                            .unwrap_or_default();
                        let remark = if current.trim().is_empty() {
                            text.clone()
                        } else {
                            format!("{current}\n{text}")
                        };
                        if self.set_remote_video_remark(*id, remark).is_some() {
                            changed += 1;
                        }
                    }
                    self.record_action(
                        "批量追加备注",
                        format!("{} 个远程视频", ids.len()),
                        ActionHistoryStatus::Succeeded,
                        changed,
                        0,
                        ids.len(),
                        started.elapsed().as_millis() as u64,
                        None,
                    );
                    self.batch_remark_buf.clear();
                    self.status_hint = format!("已为 {changed} 个远程视频追加备注");
                    return;
                }
                let result = self.service.batch_append_remark_result(&ids, &text);
                self.record_batch_action(
                    "批量追加备注",
                    format!("{} 个视频", ids.len()),
                    &result,
                    started.elapsed().as_millis() as u64,
                );
                if result.is_success() {
                    self.batch_remark_buf.clear();
                    let _ = self.reload_videos();
                    self.status_hint = format!("已为 {} 个视频追加备注", result.applied);
                } else {
                    self.error = Some(result.failures.join("\n"));
                }
            }
            if !self.all_tags.is_empty() {
                ui.label("批量应用标签");
                ui.horizontal_wrapped(|ui| {
                    for tag in &self.all_tags.clone() {
                        let mut on = self.batch_tag_ids.contains(&tag.id);
                        if ui.checkbox(&mut on, &tag.name).changed() {
                            if on {
                                if !self.batch_tag_ids.contains(&tag.id) {
                                    self.batch_tag_ids.push(tag.id);
                                }
                            } else {
                                self.batch_tag_ids.retain(|id| *id != tag.id);
                            }
                        }
                    }
                });
                if widgets::compact_secondary_button(
                    ui,
                    "应用到选中",
                    !self.batch_tag_ids.is_empty(),
                )
                .clicked()
                {
                    let ids = self.selected_ids.clone();
                    let tags = self.batch_tag_ids.clone();
                    let started = Instant::now();
                    if self.data_source == DataSource::Remote {
                        let remote_count = self.sync_remote_video_tags(&ids, &tags);
                        self.record_action(
                            "批量应用标签",
                            format!("{} 个远程视频 · {} 个标签", ids.len(), tags.len()),
                            ActionHistoryStatus::Succeeded,
                            remote_count,
                            0,
                            ids.len(),
                            started.elapsed().as_millis() as u64,
                            None,
                        );
                        self.status_hint = format!("已为 {remote_count} 个远程视频应用标签");
                        return;
                    }
                    let result = self.service.batch_set_tags_result(&ids, &tags);
                    self.record_batch_action(
                        "批量应用标签",
                        format!("{} 个视频 · {} 个标签", ids.len(), tags.len()),
                        &result,
                        started.elapsed().as_millis() as u64,
                    );
                    if result.is_success() {
                        let remote_count = self.sync_remote_video_tags(&ids, &tags);
                        self.status_hint = format!("已为 {} 个视频应用标签", result.applied);
                        if remote_count > 0 {
                            self.status_hint
                                .push_str(&format!(" · 已同步远程 {remote_count} 个"));
                        }
                        let _ = self.reload_videos();
                    } else {
                        self.error = Some(result.failures.join("\n"));
                    }
                }
            }
        }
    }

    fn info_tab_ui(&mut self, ui: &mut egui::Ui) {
        let Some(video) = self.current_video_item().cloned() else {
            ui.label("未选择视频");
            return;
        };
        let m = video.metadata();
        ui.label(format!("路径：{}", video.file_path.display()));
        ui.label(format!("时长：{}", m.duration_label()));
        ui.label(format!("分辨率：{}", m.resolution_label()));
        ui.label(format!("帧率：{:.2} fps", m.fps));
        ui.label(format!("视频编码：{}", m.video_codec));
        if let Some(ref a) = m.audio_codec {
            ui.label(format!("音频编码：{a}"));
        }
        if let Some(br) = m.bitrate_kbps {
            ui.label(format!("码率：{br} kbps"));
        }
        ui.add_space(6.0);
        ui.label("设备型号");
        if ui
            .text_edit_singleline(&mut self.device_model_buf)
            .lost_focus()
        {
            let value = self.device_model_buf.trim();
            let value = (!value.is_empty()).then_some(value);
            if self.data_source == DataSource::Remote {
                if let Some(item) = self.videos.iter_mut().find(|item| item.id == video.id) {
                    item.device_model = value.map(str::to_string);
                    item.updated_at = chrono::Utc::now();
                }
            } else if let Err(e) = self.service.update_device_model(video.id, value) {
                self.error = Some(e.to_string());
            } else {
                let _ = self.reload_videos();
            }
        }
        if m.device_model.is_none() {
            ui.label(RichText::new("未自动识别，可手动填写").weak().size(11.0));
        }
    }

    fn markers_tab_ui(&mut self, ui: &mut egui::Ui) {
        let Some(video_id) = self.current_video else {
            ui.label("未选择视频");
            return;
        };
        if self.data_source == DataSource::Remote {
            ui.label(RichText::new("远程视频暂不写入本地标记/片段。").weak());
            return;
        }
        if let Some(video) = self.current_video_item().cloned() {
            let suggestions = VideoAnalysisService::suggest_for_video(&video);
            if !suggestions.is_empty() {
                ui.label(RichText::new("规则建议").strong());
                for suggestion in suggestions.iter().take(4) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} @ {}",
                            suggestion.text,
                            format_ms(suggestion.time_ms)
                        ));
                        if let Some(tag) = &suggestion.tag_hint {
                            ui.label(RichText::new(format!("建议标签：{tag}")).small().weak());
                        }
                        if widgets::compact_secondary_button(ui, "应用为标记", true).clicked()
                        {
                            if self
                                .service
                                .add_marker(
                                    suggestion.video_id,
                                    suggestion.time_ms,
                                    MarkerKind::Issue,
                                    &suggestion.text,
                                    suggestion.severity,
                                )
                                .is_ok()
                            {
                                self.reload_markers();
                            }
                        }
                    });
                }
                ui.separator();
            }
        }
        ui.horizontal_wrapped(|ui| {
            for tpl in MARKER_TEMPLATES {
                if widgets::compact_secondary_button(ui, *tpl, true).clicked() {
                    self.new_marker_text = tpl.to_string();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("说明");
            ui.text_edit_singleline(&mut self.new_marker_text);
            if widgets::compact_primary_button(ui, "添加标记", true).clicked() {
                let t = self.compare.current_time_ms;
                if self
                    .service
                    .add_marker(video_id, t, MarkerKind::Issue, &self.new_marker_text, 2)
                    .is_ok()
                {
                    self.new_marker_text.clear();
                    self.reload_markers();
                }
            }
        });
        ui.add_space(4.0);
        ui.label("片段备注");
        ui.horizontal(|ui| {
            ui.label("起");
            let mut s = self.segment_start_ms as f64;
            if ui.add(egui::DragValue::new(&mut s).speed(100.0)).changed() {
                self.segment_start_ms = s as u64;
            }
            ui.label("止");
            let mut e = self.segment_end_ms as f64;
            if ui.add(egui::DragValue::new(&mut e).speed(100.0)).changed() {
                self.segment_end_ms = e as u64;
            }
        });
        ui.text_edit_singleline(&mut self.segment_text);
        if widgets::compact_primary_button(ui, "添加片段", true).clicked() {
            if self
                .service
                .add_segment(
                    video_id,
                    self.segment_start_ms,
                    self.segment_end_ms.max(self.segment_start_ms + 1),
                    &self.segment_text,
                    ReviewStatus::NeedsFix,
                )
                .is_ok()
            {
                self.segment_text.clear();
                self.reload_segments();
            }
        }
        ui.separator();
        for marker in &self.markers.clone() {
            ui.horizontal(|ui| {
                let jump_label = format!("{} {}", marker.kind.label(), format_ms(marker.time_ms));
                if widgets::compact_secondary_button(ui, &jump_label, true).clicked() {
                    self.compare.current_time_ms = marker.time_ms;
                }
                ui.label(&marker.text);
                if widgets::compact_secondary_button(ui, "删", true).clicked() {
                    self.pending_delete_marker = Some(marker.id);
                }
            });
        }
        if let Some(id) = self.pending_delete_marker {
            ui.horizontal(|ui| {
                ui.label("确认删除标记？");
                if widgets::compact_primary_button(ui, "确认", true).clicked() {
                    let _ = self.service.delete_marker(id);
                    self.pending_delete_marker = None;
                    self.reload_markers();
                }
                if widgets::compact_secondary_button(ui, "取消", true).clicked() {
                    self.pending_delete_marker = None;
                }
            });
        }
        ui.separator();
        for seg in &self.segments.clone() {
            ui.horizontal(|ui| {
                let jump_label =
                    format!("[{} - {}]", format_ms(seg.start_ms), format_ms(seg.end_ms));
                if widgets::compact_secondary_button(ui, &jump_label, true).clicked() {
                    self.compare.current_time_ms = seg.start_ms;
                }
                ui.label(&seg.text);
                if widgets::compact_secondary_button(ui, "删", true).clicked() {
                    self.pending_delete_segment = Some(seg.id);
                }
            });
        }
        if let Some(id) = self.pending_delete_segment {
            ui.horizontal(|ui| {
                ui.label("确认删除片段？");
                if widgets::compact_primary_button(ui, "确认", true).clicked() {
                    let _ = self.service.delete_segment(id);
                    self.pending_delete_segment = None;
                    self.reload_segments();
                }
                if widgets::compact_secondary_button(ui, "取消", true).clicked() {
                    self.pending_delete_segment = None;
                }
            });
        }
    }

    fn tags_tab_ui(&mut self, ui: &mut egui::Ui) {
        let Some(video_id) = self.current_video else {
            ui.label("未选择视频");
            return;
        };
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.new_tag_name);
            if widgets::compact_primary_button(ui, "新建", !self.new_tag_name.trim().is_empty())
                .clicked()
            {
                let color = VideoTag::PALETTE[self.new_tag_color_idx % VideoTag::PALETTE.len()];
                if self
                    .service
                    .create_tag(self.new_tag_name.trim(), color)
                    .is_ok()
                {
                    self.new_tag_name.clear();
                    self.new_tag_color_idx += 1;
                    let _ = self.reload_tags();
                }
            }
        });
        ui.separator();
        for tag in &self.all_tags.clone() {
            let mut on = self.current_tag_ids.contains(&tag.id);
            let c = tag.color;
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]), "■");
                if ui.checkbox(&mut on, &tag.name).changed() {
                    if on {
                        if !self.current_tag_ids.contains(&tag.id) {
                            self.current_tag_ids.push(tag.id);
                        }
                    } else {
                        self.current_tag_ids.retain(|id| *id != tag.id);
                    }
                    if self.data_source == DataSource::Remote {
                        let names = self.remote_video_tag_names(&self.current_tag_ids);
                        if let Some(note) = self.set_remote_video_tags(
                            video_id,
                            self.current_tag_ids.clone(),
                            names,
                        ) {
                            self.status_hint = note;
                        }
                    } else if self
                        .service
                        .set_video_tags(video_id, &self.current_tag_ids)
                        .is_ok()
                    {
                        let names = self.remote_video_tag_names(&self.current_tag_ids);
                        let _ = self.sync_remote_video_item(video_id, None, None, Some(names));
                    }
                }
            });
        }
    }

    fn export_tab_ui(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let Some(batch_id) = self.current_batch else {
            ui.label("请先选择批次");
            return;
        };

        let avail = self.service.availability();
        if !avail.ffmpeg_ok {
            widgets::warning_banner(
                ui,
                "ffmpeg 不可用，无法导出宫格图片或拼接视频。请安装 ffmpeg 并加入 PATH。",
            );
            ui.add_space(4.0);
        }

        self.defects_section_ui(ui);
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        let n = self.selected_ids.len();
        let time_ms = self.compare.current_time_ms;
        if n >= 2 && n <= MAX_COMPARE_VIDEOS {
            let layout = compute_layout(n, 480, 270);
            let (rows, cols) = grid_dimensions(n);
            ui.label(RichText::new("对比宫格预览").strong());
            ui.label(format!(
                "视频：{} 个 · 布局 {}×{} · 时间 {}",
                n,
                rows,
                cols,
                format_ms(time_ms)
            ));
            ui.label(format!(
                "输出约 {}×{} px（单格 {}×{}）",
                layout.sheet_w, layout.sheet_h, layout.cell_w, layout.cell_h
            ));
            ui.label(
                RichText::new("包含：封面帧、文件名、状态、时间、偏移、分辨率、fps")
                    .weak()
                    .size(11.0),
            );
        } else if n > MAX_COMPARE_VIDEOS {
            ui.colored_label(
                theme::error_color(ui.visuals().dark_mode),
                format!("已选 {n} 个视频，最多支持 {MAX_COMPARE_VIDEOS} 个"),
            );
        } else {
            ui.label(RichText::new("勾选 2–6 个视频后可导出对比宫格").weak());
        }

        let can_export = n >= 2 && n <= MAX_COMPARE_VIDEOS && avail.ffmpeg_ok;
        if widgets::compact_primary_button(ui, "导出当前对比宫格…", can_export).clicked()
        {
            self.export_contact_sheet();
        }

        if let Some(msg) = &self.export_success {
            ui.colored_label(
                theme::success_color(ui.visuals().dark_mode),
                format!("✓ {msg}"),
            );
        }

        ui.add_space(8.0);
        ui.label(RichText::new("对比拼接视频").strong());
        if n >= 2 && n <= MAX_COMPARE_VIDEOS {
            let (rows, cols) = grid_dimensions(n);
            let max_clip_ms = self.max_export_clip_ms();
            let max_clip_secs = max_clip_ms as f32 / 1000.0;
            let videos: Vec<VideoItem> = self
                .selected_ids
                .iter()
                .filter_map(|id| self.videos.iter().find(|v| v.id == *id).cloned())
                .collect();
            let (cell_w, cell_h) = compute_quality_cell_size(&videos);
            let output_cell_h = cell_h + self.export_caption_mode.footer_height();
            ui.label(format!(
                "布局 {}×{} · 输出 {}×{} px（单格 {}×{}，源分辨率）· 从 {} 起",
                rows,
                cols,
                cols as u32 * cell_w,
                rows as u32 * output_cell_h,
                cell_w,
                output_cell_h,
                format_ms(time_ms)
            ));
            ui.horizontal(|ui| {
                ui.label("片段时长");
                ui.add(
                    egui::Slider::new(&mut self.export_clip_secs, 1.0..=max_clip_secs.max(1.0))
                        .suffix("s")
                        .smart_aim(true),
                );
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.export_lossless, "无损导出");
                if self.export_lossless {
                    ui.label(
                        RichText::new("（CRF 0，文件较大，音轨直接复制）")
                            .weak()
                            .size(11.0),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("拼接备注");
                egui::ComboBox::from_id_salt("grid_video_caption_mode")
                    .selected_text(self.export_caption_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in GridVideoCaptionMode::all() {
                            ui.selectable_value(&mut self.export_caption_mode, mode, mode.label());
                        }
                    });
            });
            ui.label(
                RichText::new(format!(
                    "最长可导出 {:.1}s（受最短素材剩余时长限制）",
                    max_clip_secs
                ))
                .weak()
                .size(11.0),
            );
            let quality_hint = if self.export_lossless {
                "无损模式：源分辨率拼格，H.264 CRF 0，尽量保持清晰度与色彩"
            } else {
                "高质量模式：源分辨率拼格，不放大；仅必要时 Lanczos 缩小，CRF 17"
            };
            ui.label(RichText::new(quality_hint).weak().size(11.0));
        } else {
            ui.label(RichText::new("勾选 2–6 个视频后可导出拼接视频").weak());
        }

        let can_export_video = n >= 2 && n <= MAX_COMPARE_VIDEOS && avail.ffmpeg_ok;
        if widgets::compact_primary_button(ui, "导出对比拼接视频…", can_export_video).clicked()
        {
            self.export_compare_grid_video();
        }

        ui.separator();
        self.batch_screenshot_ui(ctx, ui, avail.ffmpeg_ok);

        ui.separator();
        let schema = self.video_export_schema();
        let preview_request = self.video_export_request(batch_id, PathBuf::new());
        let preview_rows = VideoExportService::preview_rows(self.service.repo(), &preview_request)
            .map(|rows| rows.len())
            .unwrap_or(0);
        ui.label(
            RichText::new(format!(
                "结构化导出预览：{} 行 · {} 列{}",
                preview_rows,
                schema.columns.iter().filter(|c| c.enabled).count(),
                if self.selected_ids.is_empty() {
                    ""
                } else {
                    "（仅选中）"
                }
            ))
            .weak()
            .size(11.0),
        );
        self.video_export_controls_ui(ui, &schema);
        if widgets::compact_secondary_button(ui, "导出 CSV…", true).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("video_review.csv")
                .save_file()
            {
                let started = Instant::now();
                let request = self.video_export_request(batch_id, path);
                match VideoExportService::export_csv(self.service.repo(), &request) {
                    Ok(r) => {
                        self.output.status_message =
                            format!("已导出 CSV（{} 行）→ {}", r.row_count, r.dest.display());
                        self.record_action(
                            "导出 CSV",
                            r.dest.display().to_string(),
                            ActionHistoryStatus::Succeeded,
                            r.row_count,
                            0,
                            r.row_count,
                            started.elapsed().as_millis() as u64,
                            Some(format!(
                                "{} 列",
                                request.schema.columns.iter().filter(|c| c.enabled).count()
                            )),
                        );
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.record_action(
                            "导出 CSV",
                            request.dest.display().to_string(),
                            ActionHistoryStatus::Failed,
                            0,
                            1,
                            1,
                            started.elapsed().as_millis() as u64,
                            Some(msg.clone()),
                        );
                        self.error = Some(msg);
                    }
                }
            }
        }
        if widgets::compact_secondary_button(ui, "导出 JSON…", true).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("video_review.json")
                .save_file()
            {
                let started = Instant::now();
                let request = self.video_export_request(batch_id, path);
                match VideoExportService::export_json_with_request(self.service.repo(), &request) {
                    Ok(()) => {
                        self.output.status_message =
                            format!("已导出 JSON → {}", request.dest.display());
                        self.record_action(
                            "导出 JSON",
                            request.dest.display().to_string(),
                            ActionHistoryStatus::Succeeded,
                            preview_rows,
                            0,
                            preview_rows,
                            started.elapsed().as_millis() as u64,
                            Some("包含 schema 与预览行".into()),
                        );
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.record_action(
                            "导出 JSON",
                            request.dest.display().to_string(),
                            ActionHistoryStatus::Failed,
                            0,
                            1,
                            1,
                            started.elapsed().as_millis() as u64,
                            Some(msg.clone()),
                        );
                        self.error = Some(msg);
                    }
                }
            }
        }
        if widgets::compact_secondary_button(ui, "导出 HTML 报告…", true).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("video_review_report.html")
                .save_file()
            {
                let started = Instant::now();
                let request = self.video_export_request(batch_id, path);
                match VideoExportService::export_html_report(self.service.repo(), &request) {
                    Ok(r) => {
                        self.output.status_message = format!(
                            "已导出 HTML 报告（{} 行）→ {}",
                            r.row_count,
                            r.dest.display()
                        );
                        self.record_action(
                            "导出 HTML 报告",
                            r.dest.display().to_string(),
                            ActionHistoryStatus::Succeeded,
                            r.row_count,
                            0,
                            r.row_count,
                            started.elapsed().as_millis() as u64,
                            Some("包含当前导出列和结构化标记".into()),
                        );
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.record_action(
                            "导出 HTML 报告",
                            request.dest.display().to_string(),
                            ActionHistoryStatus::Failed,
                            0,
                            1,
                            1,
                            started.elapsed().as_millis() as u64,
                            Some(msg.clone()),
                        );
                        self.error = Some(msg);
                    }
                }
            }
        }

        ui.add_space(8.0);
        if let Ok(stats) = self.service.frame_cache_stats() {
            ui.label(format!(
                "抽帧缓存：{} 个文件，{:.1} MB，待处理 {}",
                stats.file_count,
                stats.total_bytes as f64 / 1_048_576.0,
                stats.pending_count
            ));
        }
        if widgets::compact_secondary_button(ui, "清理抽帧缓存", true).clicked() {
            match self.service.clear_frame_cache() {
                Ok(n) => self.output.status_message = format!("已清理 {n} 个缓存文件"),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    fn batch_screenshot_ui(&mut self, ctx: &Context, ui: &mut egui::Ui, ffmpeg_ok: bool) {
        ui.label(RichText::new("批量截图").strong());
        let target_count = self.screenshot_target_videos().len();
        ui.label(format!(
            "目标 {} 个视频 · 当前时间 {}",
            target_count,
            format_ms(self.compare.current_time_ms)
        ));
        ui.horizontal_wrapped(|ui| {
            if widgets::toggle_chip(ui, "选中视频", !self.screenshot_use_filtered, true) {
                self.screenshot_use_filtered = false;
            }
            if widgets::toggle_chip(ui, "当前筛选", self.screenshot_use_filtered, true) {
                self.screenshot_use_filtered = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("模式");
            egui::ComboBox::from_id_salt("batch_screenshot_mode")
                .selected_text(self.screenshot_mode.label())
                .show_ui(ui, |ui| {
                    for mode in ScreenshotMode::all() {
                        ui.selectable_value(&mut self.screenshot_mode, mode, mode.label());
                    }
                });
        });
        if self.screenshot_mode == ScreenshotMode::Interval {
            ui.horizontal(|ui| {
                ui.label("间隔");
                ui.add(
                    egui::Slider::new(&mut self.screenshot_interval_secs, 0.5..=60.0)
                        .suffix("s")
                        .logarithmic(true),
                );
            });
        }
        ui.horizontal(|ui| {
            ui.label("上限");
            ui.add(
                egui::DragValue::new(&mut self.screenshot_max_shots)
                    .range(1..=5000)
                    .suffix(" 张"),
            );
            ui.label("格式");
            egui::ComboBox::from_id_salt("batch_screenshot_format")
                .selected_text(self.screenshot_format.extension().to_uppercase())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.screenshot_format, ScreenshotFormat::Jpeg, "JPG");
                    ui.selectable_value(&mut self.screenshot_format, ScreenshotFormat::Png, "PNG");
                });
        });
        ui.checkbox(&mut self.screenshot_write_json, "同时导出 JSON 清单");
        ui.checkbox(
            &mut self.screenshot_write_contact_sheet,
            "生成索引图（PNG，每页最多 36 张，超出自动分页）",
        );
        ui.label(
            RichText::new("默认生成 CSV 清单；失败项不会中断整个批次")
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

        let can_export = ffmpeg_ok && target_count > 0 && !self.screenshot_job.is_running();
        if widgets::compact_primary_button(ui, "批量导出截图…", can_export).clicked() {
            self.export_batch_screenshots(ctx);
        } else if !ffmpeg_ok {
            ui.label(RichText::new("需要 ffmpeg 才能批量截图").weak().size(11.0));
        } else if target_count == 0 {
            ui.label(RichText::new("请先选择或筛选视频").weak().size(11.0));
        }
    }

    fn screenshot_target_videos(&self) -> Vec<VideoItem> {
        if self.screenshot_use_filtered {
            self.videos.clone()
        } else {
            self.selected_ids
                .iter()
                .filter_map(|id| self.videos.iter().find(|v| v.id == *id).cloned())
                .collect()
        }
    }

    fn build_screenshot_request(&self, output_dir: PathBuf) -> BatchScreenshotRequest {
        let videos = self.screenshot_target_videos();
        let ids: Vec<i64> = videos.iter().map(|v| v.id).collect();
        let mut request = BatchScreenshotRequest::new(videos, self.screenshot_mode, output_dir);
        request.current_time_ms = self.compare.current_time_ms;
        request.interval_secs = self.screenshot_interval_secs as f64;
        request.max_shots = self.screenshot_max_shots as usize;
        request.format = self.screenshot_format;
        request.write_json_manifest = self.screenshot_write_json;
        request.write_contact_sheet = self.screenshot_write_contact_sheet;
        if self.screenshot_mode == ScreenshotMode::Markers {
            request.markers_by_video = self.service.markers_for_videos(&ids).unwrap_or_default();
        }
        if self.screenshot_mode == ScreenshotMode::SegmentStartEnd {
            request.segments_by_video = self.service.segments_for_videos(&ids).unwrap_or_default();
        }
        request
    }

    fn export_batch_screenshots(&mut self, ctx: &Context) {
        self.export_success = None;
        if !self.service.availability().ffmpeg_ok {
            self.error = Some("ffmpeg 不可用，无法批量截图。请安装 ffmpeg 并加入 PATH。".into());
            return;
        }
        let videos = self.screenshot_target_videos();
        if videos.is_empty() {
            self.error = Some("没有可截图的视频".into());
            return;
        }
        if self.screenshot_job.is_running() {
            return;
        }
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            let request = self.build_screenshot_request(dir.clone());
            let total = plan_shots(&request).len().max(1);
            self.screenshot_job_started = Some(Instant::now());
            self.screenshot_job_dir = Some(dir);
            self.screenshot_job.spawn(ctx, total, move |progress| {
                let backend = Arc::new(FfmpegBackend::with_defaults());
                let frame_cache = FrameCache::new(backend).map_err(|e| e.to_string())?;
                BatchScreenshotService::export(&frame_cache, &request, Some(&*progress))
                    .map_err(|e| e.to_string())
            });
        }
    }

    fn poll_screenshot_job(&mut self, ctx: &Context) {
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
        let videos_len = self.screenshot_target_videos().len();
        match result {
            Ok(result) => {
                let msg = format!(
                    "已导出截图 {} 张（成功 {} · 失败 {}）→ {}",
                    result.requested,
                    result.succeeded,
                    result.failed,
                    dir.display()
                );
                self.export_success = Some(msg.clone());
                self.output.status_message = msg;
                let status = if result.failed == 0 {
                    ActionHistoryStatus::Succeeded
                } else if result.succeeded > 0 {
                    ActionHistoryStatus::PartiallyFailed
                } else {
                    ActionHistoryStatus::Failed
                };
                self.record_action(
                    "批量导出截图",
                    dir.display().to_string(),
                    status,
                    result.succeeded,
                    result.failed,
                    result.requested,
                    started.elapsed().as_millis() as u64,
                    Some(format!(
                        "模式：{} · CSV {} · 索引 {}",
                        self.screenshot_mode.label(),
                        result
                            .csv_manifest
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "无".into()),
                        format_video_contact_sheets(&result.contact_sheets),
                    )),
                );
            }
            Err(e) => {
                let msg = e.to_string();
                self.record_action(
                    "批量导出截图",
                    dir.display().to_string(),
                    ActionHistoryStatus::Failed,
                    0,
                    videos_len,
                    videos_len,
                    started.elapsed().as_millis() as u64,
                    Some(msg.clone()),
                );
                self.error = Some(msg);
            }
        }
    }

    fn start_import_folder(&mut self, ctx: &Context, folder: PathBuf) {
        if self.import_job.is_running() {
            return;
        }
        self.error = None;
        self.import_job_started = Some(Instant::now());
        self.import_job_folder = Some(folder.clone());
        self.import_job.spawn(ctx, 1, move |progress| {
            let service = VideoReviewService::open().map_err(|e| e.to_string())?;
            service
                .import_folder_with_options(
                    &folder,
                    None,
                    ImportFolderOptions::fast(),
                    Some(progress.as_ref()),
                )
                .map_err(|e| e.to_string())
        });
        self.status_hint = "正在导入视频…".into();
    }

    fn poll_import_job(&mut self, ctx: &Context) {
        let Some(result) = self.import_job.poll(ctx) else {
            return;
        };
        let started = self
            .import_job_started
            .take()
            .unwrap_or_else(Instant::now);
        let folder = self
            .import_job_folder
            .take()
            .unwrap_or_else(|| PathBuf::from("."));
        self.status_hint.clear();
        match result {
            Ok(r) => {
                self.current_batch = Some(r.batch_id);
                let _ = self.reload_batches();
                let remote_paths = video_paths_for_folder(&folder).unwrap_or_default();
                let remote_note = self.sync_remote_video_batch(
                    folder
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("视频批次"),
                    &remote_paths,
                );
                let mut msg = format!(
                    "已导入 {} 个视频（跳过 {}）→ {}",
                    r.imported,
                    r.skipped.len(),
                    folder.display()
                );
                if let Some(note) = remote_note {
                    msg.push_str(&format!(" · {note}"));
                }
                if !r.skipped.is_empty() {
                    let sample: Vec<_> = r
                        .skipped
                        .iter()
                        .take(2)
                        .map(|s| {
                            format!(
                                "{}: {}",
                                s.path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| s.path.display().to_string()),
                                s.reason
                            )
                        })
                        .collect();
                    self.error = Some(format!(
                        "部分视频未导入（{}）：{}",
                        r.skipped.len(),
                        sample.join("；")
                    ));
                }
                self.status_hint = msg.clone();
                self.export_success = Some(msg);
                self.record_action(
                    "导入视频文件夹",
                    folder.display().to_string(),
                    if r.skipped.is_empty() {
                        ActionHistoryStatus::Succeeded
                    } else {
                        ActionHistoryStatus::PartiallyFailed
                    },
                    r.imported,
                    r.skipped.len(),
                    r.imported + r.skipped.len(),
                    started.elapsed().as_millis() as u64,
                    None,
                );
            }
            Err(e) => {
                self.record_action(
                    "导入视频文件夹",
                    folder.display().to_string(),
                    ActionHistoryStatus::Failed,
                    0,
                    1,
                    1,
                    started.elapsed().as_millis() as u64,
                    Some(e.clone()),
                );
                self.error = Some(e);
            }
        }
    }

    fn start_frame_align(&mut self, ctx: &Context) {
        self.error = None;
        self.export_success = None;
        if !self.service.availability().ffmpeg_ok {
            self.error = Some("ffmpeg 不可用，无法帧对齐".into());
            return;
        }
        let videos = match self.selected_compare_videos() {
            Some(v) => v,
            None => return,
        };
        if videos.len() < 2 {
            self.error = Some("至少选择 2 个视频才能对齐".into());
            return;
        }
        if self.align_job.is_running() {
            return;
        }
        let around_ms = self.compare.current_time_ms;
        let reference = videos[0].clone();
        let others = videos[1..].to_vec();
        self.align_job.spawn(ctx, videos.len(), move |_progress| {
            let service = VideoReviewService::open().map_err(|e| e.to_string())?;
            service
                .align_videos(&reference, &others, Some(around_ms))
                .map_err(|e| e.to_string())
        });
        self.status_hint = "正在按音频对齐…".into();
    }

    fn poll_align_job(&mut self, ctx: &Context) {
        let Some(result) = self.align_job.poll(ctx) else {
            return;
        };
        match result {
            Ok(batch) => {
                let mut prev = HashMap::new();
                for pair in &batch.pairs {
                    if let Some(item) = self.videos.iter().find(|v| v.id == pair.video_id) {
                        prev.insert(pair.video_id, item.offset_ms);
                    }
                }
                self.align_prev_offsets = prev;
                let applied = self.apply_align_pairs(&batch.pairs, false);
                let low_conf = batch
                    .pairs
                    .iter()
                    .filter(|p| {
                        p.video_id != batch.reference_id && p.confidence < ALIGN_CONFIDENCE_WARN
                    })
                    .count();
                self.defect_align_method = "audio_xcorr".into();
                self.align_review = Some(batch.clone());
                self.align_review_open = true;
                let msg = if low_conf > 0 {
                    format!(
                        "已对齐 {applied} 路；{low_conf} 路置信度偏低，请在对齐结果中确认或微调"
                    )
                } else {
                    format!("已对齐 {applied} 路（音频互相关，已量化到整帧）")
                };
                self.export_success = Some(msg.clone());
                self.output.status_message = msg;
                self.status_hint.clear();
            }
            Err(e) => {
                self.status_hint.clear();
                self.error = Some(e);
            }
        }
    }

    fn apply_align_pairs(&mut self, pairs: &[AlignPairResult], high_conf_only: bool) -> usize {
        let mut applied = 0usize;
        for pair in pairs {
            if high_conf_only
                && pair.confidence < ALIGN_CONFIDENCE_WARN
                && self
                    .align_review
                    .as_ref()
                    .map(|b| b.reference_id != pair.video_id)
                    .unwrap_or(true)
            {
                // 低置信度：恢复对齐前偏移
                let prev = self
                    .align_prev_offsets
                    .get(&pair.video_id)
                    .copied()
                    .unwrap_or(pair.offset_ms);
                let _ = self.service.update_offset(pair.video_id, prev);
                if let Some(item) = self.videos.iter_mut().find(|v| v.id == pair.video_id) {
                    item.offset_ms = prev;
                }
                continue;
            }
            if let Err(e) = self.service.update_offset(pair.video_id, pair.offset_ms) {
                self.error = Some(format!("写入偏移失败: {e}"));
                continue;
            }
            if let Some(item) = self.videos.iter_mut().find(|v| v.id == pair.video_id) {
                item.offset_ms = pair.offset_ms;
            }
            applied += 1;
        }
        applied
    }

    fn undo_align_offsets(&mut self) {
        if self.align_prev_offsets.is_empty() {
            return;
        }
        for (id, offset) in self.align_prev_offsets.clone() {
            let _ = self.service.update_offset(id, offset);
            if let Some(item) = self.videos.iter_mut().find(|v| v.id == id) {
                item.offset_ms = offset;
            }
        }
        self.align_prev_offsets.clear();
        self.align_review = None;
        self.align_review_open = false;
        self.defect_align_method = "manual".into();
        let msg = "已撤销上次对齐偏移".to_string();
        self.export_success = Some(msg.clone());
        self.output.status_message = msg;
    }

    fn apply_compare_nudges(&mut self, nudges: &[(i64, i64)]) {
        for &(video_id, frames) in nudges {
            let Some(item) = self.videos.iter().find(|v| v.id == video_id).cloned() else {
                continue;
            };
            let new_offset = offset_after_frame_step(item.offset_ms, item.fps, frames);
            if let Err(e) = self.service.update_offset(video_id, new_offset) {
                self.error = Some(format!("微调偏移失败: {e}"));
                continue;
            }
            if let Some(v) = self.videos.iter_mut().find(|v| v.id == video_id) {
                v.offset_ms = new_offset;
            }
            self.defect_align_method = "manual_frame".into();
        }
    }

    fn draw_align_review(&mut self, ctx: &Context) {
        if !self.align_review_open {
            return;
        }
        let Some(batch) = self.align_review.clone() else {
            self.align_review_open = false;
            return;
        };
        let mut open = self.align_review_open;
        let mut apply_high = false;
        let mut undo = false;
        let mut close = false;
        let mut nudge: Option<(i64, i64)> = None;
        egui::Window::new("对齐结果")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "主视频 id={} · 低于 {:.0}% 置信度建议人工确认",
                        batch.reference_id,
                        ALIGN_CONFIDENCE_WARN * 100.0
                    ))
                    .weak(),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for pair in &batch.pairs {
                            let name = self
                                .videos
                                .iter()
                                .find(|v| v.id == pair.video_id)
                                .and_then(|v| {
                                    v.file_path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                })
                                .unwrap_or_else(|| format!("#{}", pair.video_id));
                            let low = pair.video_id != batch.reference_id
                                && pair.confidence < ALIGN_CONFIDENCE_WARN;
                            ui.horizontal(|ui| {
                                if low {
                                    ui.colored_label(Color32::from_rgb(200, 120, 40), "⚠");
                                } else {
                                    ui.label("✓");
                                }
                                ui.label(RichText::new(name).strong().size(12.0));
                                ui.label(format!("{}ms", pair.offset_ms));
                                ui.label(
                                    RichText::new(format!("置信 {:.0}%", pair.confidence * 100.0))
                                        .weak()
                                        .size(11.0),
                                );
                                if pair.video_id != batch.reference_id {
                                    if ui.small_button("−1帧").clicked() {
                                        nudge = Some((pair.video_id, 1));
                                    }
                                    if ui.small_button("+1帧").clicked() {
                                        nudge = Some((pair.video_id, -1));
                                    }
                                }
                            });
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::compact_secondary_button(ui, "仅保留高置信度", true).clicked() {
                        apply_high = true;
                    }
                    if widgets::compact_secondary_button(ui, "撤销对齐", true).clicked() {
                        undo = true;
                    }
                    if widgets::compact_primary_button(ui, "完成", true).clicked() {
                        close = true;
                    }
                });
            });
        if let Some((id, frames)) = nudge {
            self.apply_compare_nudges(&[(id, frames)]);
            if let Some(review) = self.align_review.as_mut() {
                if let Some(pair) = review.pairs.iter_mut().find(|p| p.video_id == id) {
                    if let Some(v) = self.videos.iter().find(|v| v.id == id) {
                        pair.offset_ms = v.offset_ms;
                    }
                }
            }
        }
        if apply_high {
            let n = self.apply_align_pairs(&batch.pairs, true);
            self.export_success = Some(format!("已仅保留高置信度偏移（{n} 路）"));
        }
        if undo {
            self.undo_align_offsets();
            return;
        }
        if close || !open {
            self.align_review_open = false;
        }
    }

    fn open_defect_dialog(&mut self) {
        self.error = None;
        if self.defect_title.trim().is_empty() {
            self.defect_title = format!("缺陷 @ {}", format_ms(self.compare.current_time_ms));
        }
        self.defect_output_dir = None;
        if self.defect_align_method.is_empty() {
            self.defect_align_method = "manual".into();
        }
        self.defect_dialog_open = true;
    }

    fn draw_defect_dialog(&mut self, ctx: &Context) {
        if !self.defect_dialog_open {
            return;
        }
        let mut open = self.defect_dialog_open;
        let mut create = false;
        let mut cancel = false;
        let mut preset_full = false;
        let mut preset_light = false;
        egui::Window::new("从对比新建缺陷")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("将打包宫格图、对比片段、单帧与可选原片为 zip").weak());
                ui.horizontal(|ui| {
                    if widgets::compact_secondary_button(ui, "完整包", true)
                        .on_hover_text("原片 + 无损片段")
                        .clicked()
                    {
                        preset_full = true;
                    }
                    if widgets::compact_secondary_button(ui, "轻量包", true)
                        .on_hover_text("不含原片，高质量片段")
                        .clicked()
                    {
                        preset_light = true;
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("标题");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.defect_title).desired_width(280.0),
                    );
                });
                ui.label("描述");
                ui.add(
                    egui::TextEdit::multiline(&mut self.defect_description)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3),
                );
                ui.horizontal(|ui| {
                    ui.label("严重度");
                    ui.add(egui::Slider::new(&mut self.defect_severity, 1..=5));
                    ui.label("半窗 (秒)");
                    ui.add(
                        egui::DragValue::new(&mut self.defect_half_secs)
                            .speed(0.5)
                            .range(0.5..=60.0),
                    );
                });
                ui.checkbox(&mut self.defect_include_grid, "宫格对比图 PNG");
                ui.checkbox(&mut self.defect_include_clip, "对比拼接片段");
                ui.checkbox(&mut self.defect_include_frames, "各路单帧 JPG");
                ui.checkbox(&mut self.defect_include_sources, "打包原片（体积大，默认开启）");
                ui.checkbox(&mut self.defect_lossless, "无损画质（关闭则为高质量）");
                ui.checkbox(&mut self.defect_mark_issue, "写入 Issue 标记");
                ui.checkbox(&mut self.defect_set_needs_fix, "标记为 NeedsFix");
                ui.checkbox(
                    &mut self.defect_require_align,
                    "要求已做帧对齐（audio / 手动微调）",
                );
                ui.horizontal(|ui| {
                    let dir_label = self
                        .defect_output_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "默认：主视频所在目录".into());
                    ui.label(RichText::new(dir_label).weak().size(11.0));
                    if widgets::compact_secondary_button(ui, "选择目录…", true).clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.defect_output_dir = Some(dir);
                        }
                    }
                });
                if self.defect_job.is_running() {
                    if let Some(progress) = self.defect_job.progress() {
                        ui.add(egui::ProgressBar::new(progress.fraction()).show_percentage());
                        if let Some(label) = ProgressReporter::status_label(progress.as_ref()) {
                            ui.label(RichText::new(label).weak().size(11.0));
                        }
                    }
                    if widgets::compact_secondary_button(ui, "取消打包", true).clicked() {
                        self.defect_job.request_cancel();
                        self.status_hint = "正在取消打包…".into();
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::compact_primary_button(
                        ui,
                        "创建并打包",
                        !self.defect_title.trim().is_empty() && !self.defect_job.is_running(),
                    )
                    .clicked()
                    {
                        create = true;
                    }
                    if widgets::compact_secondary_button(ui, "关闭", !self.defect_job.is_running())
                        .clicked()
                    {
                        cancel = true;
                    }
                });
            });
        if preset_full {
            self.defect_include_grid = true;
            self.defect_include_clip = true;
            self.defect_include_frames = true;
            self.defect_include_sources = true;
            self.defect_lossless = true;
        }
        if preset_light {
            self.defect_include_grid = true;
            self.defect_include_clip = true;
            self.defect_include_frames = true;
            self.defect_include_sources = false;
            self.defect_lossless = false;
        }
        if create {
            self.start_create_defect(ctx);
        }
        if cancel || !open {
            if !self.defect_job.is_running() {
                self.defect_dialog_open = false;
            }
        } else {
            self.defect_dialog_open = open;
        }
    }

    fn start_create_defect(&mut self, ctx: &Context) {
        if self.defect_job.is_running() {
            return;
        }
        if !self.service.availability().ffmpeg_ok {
            self.error = Some("ffmpeg 不可用，无法创建缺陷包".into());
            return;
        }
        if self.defect_require_align
            && self.defect_align_method != "audio_xcorr"
            && self.defect_align_method != "manual_frame"
        {
            self.error = Some("已勾选「要求帧对齐」，请先执行帧对齐或逐帧微调".into());
            return;
        }
        let videos = match self.selected_compare_videos() {
            Some(v) => v,
            None => return,
        };
        let batch_id = self.current_batch.unwrap_or(0);
        let output_dir = self
            .defect_output_dir
            .clone()
            .unwrap_or_else(|| default_defect_output_dir(&videos));
        let half_ms = (self.defect_half_secs * 1000.0).round() as u64;
        let req = CreateDefectRequest {
            batch_id,
            title: self.defect_title.trim().to_string(),
            description: self.defect_description.clone(),
            severity: self.defect_severity,
            time_ms: self.compare.current_time_ms,
            half_window_ms: half_ms.max(500),
            videos,
            output_dir,
            include_grid_png: self.defect_include_grid,
            include_compare_clip: self.defect_include_clip,
            include_frames: self.defect_include_frames,
            include_sources: self.defect_include_sources,
            quality: if self.defect_lossless {
                GridVideoExportQuality::Lossless
            } else {
                GridVideoExportQuality::High
            },
            mark_issue: self.defect_mark_issue,
            set_needs_fix: self.defect_set_needs_fix,
            align_method: self.defect_align_method.clone(),
        };
        let total = 8 + req.videos.len() * 2;
        self.defect_job
            .spawn_with_context(ctx, total.max(1), move |job| {
                let service = VideoReviewService::open().map_err(|e| e.to_string())?;
                service
                    .create_defect(
                        req,
                        Some(job.progress.as_ref()),
                        Some(job.cancel.as_ref()),
                    )
                    .map_err(|e| e.to_string())
            });
        self.status_hint = "正在打包缺陷…".into();
    }

    fn poll_defect_job(&mut self, ctx: &Context) {
        let Some(result) = self.defect_job.poll(ctx) else {
            return;
        };
        self.status_hint.clear();
        match result {
            Ok(r) => {
                self.defect_dialog_open = false;
                let msg = format!(
                    "已创建缺陷「{}」→ {}（目录 {}）",
                    r.defect.title,
                    r.zip_path.display(),
                    r.folder.display()
                );
                self.export_success = Some(msg.clone());
                self.output.status_message = msg;
                let _ = self.reload_videos();
                self.reload_defects();
                if self.current_video.is_some() {
                    self.reload_markers();
                    self.reload_segments();
                }
            }
            Err(e) => {
                if e.contains("已取消") {
                    self.export_success = Some("已取消缺陷打包".into());
                    self.output.status_message = "已取消缺陷打包".into();
                } else {
                    self.error = Some(e);
                }
            }
        }
    }

    fn defects_section_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("缺陷包历史").strong());
            if widgets::compact_secondary_button(ui, "刷新", true).clicked() {
                self.reload_defects();
            }
        });
        if self.defects.is_empty() {
            ui.label(
                RichText::new("当前批次尚无缺陷包。可在对比模式用「从对比新建缺陷」创建。")
                    .weak()
                    .size(11.0),
            );
            return;
        }

        let jira_ready = self.jira_config.is_configured() && self.jira_config.has_credentials();
        ui.horizontal(|ui| {
            let selected_n = self.selected_defect_ids.len();
            ui.label(
                RichText::new(format!("已选 {selected_n}"))
                    .size(11.0)
                    .weak(),
            );
            let submit_ok =
                selected_n > 0 && jira_ready && !self.jira_job.is_running();
            if widgets::compact_secondary_button(ui, "批量提交 JIRA", submit_ok).clicked() {
                self.jira_dialog = Some(VideoJiraDialog::Confirm {
                    force_recreate: false,
                    attach: self.jira_config.attach_defect_zip,
                });
            }
            if self.jira_job.is_running() {
                if let Some(progress) = self.jira_job.progress() {
                    ui.add(
                        egui::ProgressBar::new(progress.fraction())
                            .desired_width(80.0)
                            .show_percentage(),
                    );
                }
                if widgets::compact_secondary_button(ui, "取消", true).clicked() {
                    self.jira_job.request_cancel();
                    self.status_hint = "正在取消 JIRA 提交…".into();
                }
            } else if !jira_ready {
                ui.label(RichText::new("JIRA 未配置").size(10.0).weak());
            }
        });

        egui::ScrollArea::vertical()
            .max_height(160.0)
            .id_salt("video_defect_history")
            .show(ui, |ui| {
                for d in self.defects.clone() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let mut checked = self.selected_defect_ids.contains(&d.id);
                            if ui.checkbox(&mut checked, "").changed() {
                                if checked {
                                    if !self.selected_defect_ids.contains(&d.id) {
                                        self.selected_defect_ids.push(d.id);
                                    }
                                } else {
                                    self.selected_defect_ids.retain(|id| *id != d.id);
                                }
                            }
                            ui.label(RichText::new(&d.title).strong().size(12.0));
                            ui.label(
                                RichText::new(format!("S{}", d.severity))
                                    .weak()
                                    .size(11.0),
                            );
                            ui.label(RichText::new(format_ms(d.time_ms)).weak().size(11.0));
                            if let Some(key) = &d.jira_issue_key {
                                if ui
                                    .link(RichText::new(key).size(11.0))
                                    .on_hover_text("打开 JIRA")
                                    .clicked()
                                {
                                    if let Some(url) = d
                                        .jira_url
                                        .clone()
                                        .or_else(|| self.jira_config.issue_browse_url(key))
                                    {
                                        let _ = open::that(url);
                                    }
                                }
                            }
                        });
                        ui.label(
                            RichText::new(format!(
                                "{} · {} 路 · ±{}s",
                                d.created_at.format("%Y-%m-%d %H:%M"),
                                d.video_ids.len(),
                                d.half_window_ms / 1000
                            ))
                            .weak()
                            .size(11.0),
                        );
                        if !d.description.is_empty() {
                            ui.label(RichText::new(&d.description).size(11.0));
                        }
                        ui.horizontal(|ui| {
                            if let Some(zip) = &d.package_path {
                                let zip = zip.clone();
                                if widgets::compact_secondary_button(ui, "打开 zip", zip.exists())
                                    .clicked()
                                {
                                    let _ = open::that(&zip);
                                }
                                if let Some(parent) = zip.parent() {
                                    let parent = parent.to_path_buf();
                                    if widgets::compact_secondary_button(ui, "打开目录", true)
                                        .clicked()
                                    {
                                        let _ = open::that(&parent);
                                    }
                                }
                            }
                            if widgets::compact_secondary_button(ui, "跳转时间", true).clicked() {
                                self.compare.current_time_ms = d.time_ms;
                                if !self.compare_mode && self.selected_ids.len() >= 2 {
                                    self.compare_mode = true;
                                    self.compare.set_compare_ids(self.selected_ids.clone());
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });
    }

    fn show_jira_dialogs(&mut self, ctx: &Context) {
        let Some(dialog) = self.jira_dialog.clone() else {
            return;
        };
        match dialog {
            VideoJiraDialog::Confirm {
                mut force_recreate,
                mut attach,
            } => {
                let count = self.selected_defect_ids.len();
                let project = self
                    .jira_config
                    .project_key
                    .clone()
                    .unwrap_or_else(|| "？".into());
                egui::Window::new("批量提交 JIRA")
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "将为已选 {count} 个缺陷包在项目 {project} 各创建 1 个 Bug。"
                        ));
                        ui.checkbox(&mut attach, "上传缺陷 zip 附件");
                        ui.checkbox(&mut force_recreate, "已有关联 Issue 时仍强制新建");
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, "开始提交", true).clicked() {
                                self.start_jira_submit(ctx, force_recreate, attach);
                                self.jira_dialog = None;
                            }
                            if widgets::secondary_button(ui, "取消", true).clicked() {
                                self.jira_dialog = None;
                            }
                        });
                    });
                if self.jira_dialog.is_some() {
                    self.jira_dialog = Some(VideoJiraDialog::Confirm {
                        force_recreate,
                        attach,
                    });
                }
            }
            VideoJiraDialog::Result { summary, lines } => {
                egui::Window::new("JIRA 提交结果")
                    .collapsible(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .default_width(420.0)
                    .show(ctx, |ui| {
                        ui.label(RichText::new(&summary).strong());
                        ui.add_space(6.0);
                        ScrollArea::vertical()
                            .id_salt("video_jira_result")
                            .max_height(220.0)
                            .show(ui, |ui| {
                                for line in &lines {
                                    if let Some(url) = &line.browse_url {
                                        if ui
                                            .link(RichText::new(&line.text).size(12.0))
                                            .on_hover_text(url)
                                            .clicked()
                                        {
                                            let _ = open::that(url);
                                        }
                                    } else {
                                        ui.label(RichText::new(&line.text).size(12.0));
                                    }
                                }
                            });
                        if widgets::primary_button(ui, "关闭", true).clicked() {
                            self.jira_dialog = None;
                        }
                    });
            }
        }
    }

    fn start_jira_submit(&mut self, ctx: &Context, force_recreate: bool, attach: bool) {
        if self.jira_job.is_running() {
            return;
        }
        let selected: Vec<VideoDefect> = self
            .defects
            .iter()
            .filter(|d| self.selected_defect_ids.contains(&d.id))
            .cloned()
            .collect();
        if selected.is_empty() {
            self.error = Some("请先勾选缺陷包".into());
            return;
        }
        let mut jira_cfg = self.jira_config.clone();
        jira_cfg.apply_env_overrides();
        let total = selected.len();
        self.status_hint = "正在提交 JIRA…".into();
        self.jira_job
            .spawn_with_context(ctx, total, move |job| {
                let progress = job.progress;
                let options = crate::jira::JiraBatchOptions {
                    force_recreate,
                    attach,
                };
                let service = crate::jira::JiraIssueService::try_new(&jira_cfg)
                    .map_err(|e| e.to_string())?;
                let mut result = service
                    .batch_create_from_defects(
                        &selected,
                        &options,
                        Some(&|done, tot, label| {
                            progress
                                .completed
                                .store(done, std::sync::atomic::Ordering::Relaxed);
                            progress
                                .total
                                .store(tot, std::sync::atomic::Ordering::Relaxed);
                            progress.set_current_label(label);
                        }),
                        Some(job.cancel.as_ref()),
                    )
                    .map_err(|e| e.to_string())?;

                let svc = VideoReviewService::open().map_err(|e| e.to_string())?;
                for item in &mut result.items {
                    if let Some(key) = &item.issue_key {
                        if !item.skipped && item.error.is_none() {
                            if let Err(e) = svc.update_defect_jira(
                                item.local_id,
                                key,
                                item.browse_url.as_deref(),
                            ) {
                                item.persist_warning =
                                    Some(format!("已建单但本地未关联：{e}"));
                            }
                        }
                    }
                }
                Ok(result)
            });
    }

    fn poll_jira_job(&mut self, ctx: &Context) {
        let Some(result) = self.jira_job.poll(ctx) else {
            return;
        };
        match result {
            Ok(result) => {
                let summary = result.summary_line();
                let lines: Vec<VideoJiraResultLine> = result
                    .items
                    .iter()
                    .map(|i| {
                        let (text, browse_url) = if i.skipped {
                            (
                                format!(
                                    "#{} 跳过：{}",
                                    i.local_id,
                                    i.skip_reason.as_deref().unwrap_or("已关联")
                                ),
                                i.browse_url.clone(),
                            )
                        } else if let Some(err) = &i.error {
                            (format!("#{} 失败：{err}", i.local_id), None)
                        } else if let Some(key) = &i.issue_key {
                            let mut warn = String::new();
                            if let Some(w) = &i.attachment_warning {
                                warn.push_str(&format!("（{w}）"));
                            }
                            if let Some(w) = &i.persist_warning {
                                warn.push_str(&format!("（{w}）"));
                            }
                            (format!("#{} → {key}{warn}", i.local_id), i.browse_url.clone())
                        } else {
                            (format!("#{} 未知结果", i.local_id), None)
                        };
                        VideoJiraResultLine { text, browse_url }
                    })
                    .collect();
                self.status_hint = summary.clone();
                self.reload_defects();
                self.jira_dialog = Some(VideoJiraDialog::Result { summary, lines });
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }

    fn reload_defects(&mut self) {
        self.defects = self
            .current_batch
            .and_then(|id| self.service.list_defects(id).ok())
            .unwrap_or_default();
        self.selected_defect_ids
            .retain(|id| self.defects.iter().any(|d| d.id == *id));
    }

    fn selected_compare_videos(&mut self) -> Option<Vec<VideoItem>> {
        if self.selected_ids.len() < 2 {
            self.error = Some("请至少选择 2 个视频".into());
            return None;
        }
        if self.selected_ids.len() > MAX_COMPARE_VIDEOS {
            self.error = Some(format!("最多选择 {MAX_COMPARE_VIDEOS} 个视频进行对比导出"));
            return None;
        }
        let videos: Vec<VideoItem> = self
            .selected_ids
            .iter()
            .filter_map(|id| self.videos.iter().find(|v| v.id == *id).cloned())
            .collect();
        if videos.len() < 2 {
            self.error = Some("未找到足够的对比视频".into());
            return None;
        }
        Some(videos)
    }

    fn max_export_clip_ms(&self) -> u64 {
        let videos: Vec<VideoItem> = self
            .selected_ids
            .iter()
            .filter_map(|id| self.videos.iter().find(|v| v.id == *id).cloned())
            .collect();
        max_export_duration_ms(&videos, self.compare.current_time_ms)
    }

    fn export_contact_sheet(&mut self) {
        self.export_success = None;
        let avail = self.service.availability();
        if !avail.ffmpeg_ok {
            self.error = Some("ffmpeg 不可用，无法抽帧。请安装 ffmpeg 并加入 PATH 后重启。".into());
            return;
        }
        let videos = match self.selected_compare_videos() {
            Some(v) => v,
            None => return,
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("video_compare_grid.png")
            .save_file()
        {
            let started = Instant::now();
            let time_ms = self.compare.current_time_ms;
            match self
                .service
                .export_compare_contact_sheet(&videos, time_ms, path.clone())
            {
                Ok(r) => {
                    let msg = format!(
                        "已导出宫格 {}×{}（{} 个视频）→ {}",
                        r.width,
                        r.height,
                        r.video_count,
                        r.dest.display()
                    );
                    self.export_success = Some(msg.clone());
                    self.output.status_message = msg;
                    self.record_action(
                        "导出对比宫格",
                        r.dest.display().to_string(),
                        ActionHistoryStatus::Succeeded,
                        r.video_count,
                        0,
                        r.video_count,
                        started.elapsed().as_millis() as u64,
                        Some(format!("{}×{} @ {}", r.width, r.height, format_ms(time_ms))),
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.record_action(
                        "导出对比宫格",
                        path.display().to_string(),
                        ActionHistoryStatus::Failed,
                        0,
                        videos.len(),
                        videos.len(),
                        started.elapsed().as_millis() as u64,
                        Some(msg.clone()),
                    );
                    if msg.contains("ffmpeg") || msg.contains("抽帧") {
                        self.error = Some(format!("抽帧失败：{msg}"));
                    } else if msg.contains("save")
                        || msg.contains("写入")
                        || msg.contains("permission")
                    {
                        self.error = Some(format!("保存失败：{msg}"));
                    } else {
                        self.error = Some(msg);
                    }
                }
            }
        }
    }

    fn export_compare_grid_video(&mut self) {
        self.export_success = None;
        let avail = self.service.availability();
        if !avail.ffmpeg_ok {
            self.error =
                Some("ffmpeg 不可用，无法导出拼接视频。请安装 ffmpeg 并加入 PATH 后重启。".into());
            return;
        }
        let videos = match self.selected_compare_videos() {
            Some(v) => v,
            None => return,
        };
        let start_ms = self.compare.current_time_ms;
        let duration_ms = ((self.export_clip_secs * 1000.0) as u64).min(self.max_export_clip_ms());
        if duration_ms < 500 {
            self.error = Some("当前时间点之后没有足够时长可导出".into());
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MP4 视频", &["mp4"])
            .set_file_name(if self.export_lossless {
                "video_compare_grid_lossless.mp4"
            } else {
                "video_compare_grid.mp4"
            })
            .save_file()
        {
            let started = Instant::now();
            let quality = if self.export_lossless {
                GridVideoExportQuality::Lossless
            } else {
                GridVideoExportQuality::High
            };
            match self.service.export_compare_grid_video(
                &videos,
                start_ms,
                duration_ms,
                path.clone(),
                quality,
                self.export_caption_mode,
            ) {
                Ok(r) => {
                    let msg = format!(
                        "已导出{}拼接视频 {}×{} · {:.1}s（单格 {}×{}，{} 路）→ {}",
                        if r.quality == GridVideoExportQuality::Lossless {
                            "无损"
                        } else {
                            ""
                        },
                        r.width,
                        r.height,
                        r.duration_ms as f64 / 1000.0,
                        r.cell_width,
                        r.cell_height,
                        r.video_count,
                        r.dest.display()
                    );
                    self.export_success = Some(msg.clone());
                    self.output.status_message = msg;
                    if let Some(warning) = &r.caption_warning {
                        self.error = Some(warning.clone());
                    }
                    self.record_action(
                        "导出对比拼接视频",
                        r.dest.display().to_string(),
                        ActionHistoryStatus::Succeeded,
                        r.video_count,
                        0,
                        r.video_count,
                        started.elapsed().as_millis() as u64,
                        Some(format!(
                            "{}×{} · {:.1}s",
                            r.width,
                            r.height,
                            r.duration_ms as f64 / 1000.0
                        )),
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.record_action(
                        "导出对比拼接视频",
                        path.display().to_string(),
                        ActionHistoryStatus::Failed,
                        0,
                        videos.len(),
                        videos.len(),
                        started.elapsed().as_millis() as u64,
                        Some(msg.clone()),
                    );
                    if msg.contains("ffmpeg") || msg.contains("视频导出") {
                        self.error = Some(format!("视频导出失败：{msg}"));
                    } else if msg.contains("permission") || msg.contains("写入") {
                        self.error = Some(format!("保存失败：{msg}"));
                    } else {
                        self.error = Some(msg);
                    }
                }
            }
        }
    }

    fn select_video(&mut self, id: i64) {
        self.current_video = Some(id);
        if self.stack_pane == VideoStackPane::List {
            self.stack_pane = VideoStackPane::Player;
        }
        self.ensure_remote_original_downloaded(id);
        if self.data_source == DataSource::Remote {
            if let Some(v) = self.videos.iter().find(|v| v.id == id).cloned() {
                self.remark_buf = v.remark.clone().unwrap_or_default();
                self.offset_buf = v.offset_ms.to_string();
                self.device_model_buf = v.device_model.clone().unwrap_or_default();
                self.timeline_thumbs.clear();
                self.current_tag_ids = self
                    .remote_item_ids
                    .get(&id)
                    .and_then(|remote_item_id| {
                        self.remote_items
                            .iter()
                            .find(|item| item.item_id == *remote_item_id)
                    })
                    .map(|item| self.tag_ids_for_remote_names(&item.tags))
                    .unwrap_or_default();
                self.video_tag_map.insert(id, self.current_tag_ids.clone());
                self.markers.clear();
                self.segments.clear();
            }
        } else if let Ok(v) = self.service.get_video(id) {
            self.remark_buf = v.remark.clone().unwrap_or_default();
            self.offset_buf = v.offset_ms.to_string();
            self.device_model_buf = v.device_model.clone().unwrap_or_default();
            self.timeline_thumbs.clear();
            if let Ok(ids) = self.service.get_video_tag_ids(id) {
                self.current_tag_ids = ids;
            }
            self.reload_markers();
            self.reload_segments();
        }
    }

    fn current_video_item(&self) -> Option<&VideoItem> {
        self.current_video
            .and_then(|id| self.videos.iter().find(|v| v.id == id))
    }

    fn reload_batches(&mut self) -> Result<(), String> {
        if self.data_source == DataSource::Remote {
            self.reload_tags()?;
            self.start_remote_batches_fetch();
            return Ok(());
        }
        self.batches = self.service.list_batches().map_err(|e| e.to_string())?;
        if self.current_batch.is_none() {
            self.current_batch = self.batches.first().map(|b| b.id);
        }
        self.reload_videos()?;
        self.reload_tags()?;
        Ok(())
    }

    fn reload_videos(&mut self) -> Result<(), String> {
        let Some(batch_id) = self.current_batch else {
            self.videos.clear();
            self.current_video = None;
            self.defects.clear();
            return Ok(());
        };
        if self.data_source == DataSource::Remote {
            if let Some(remote_batch_id) =
                self.remote_id_map.remote_of(batch_id).map(str::to_string)
            {
                self.start_remote_items_fetch(remote_batch_id);
            } else {
                self.videos.clear();
                self.current_video = None;
            }
            return Ok(());
        }
        self.videos = self
            .service
            .list_videos(batch_id, &self.video_list_state.filter)
            .map_err(|e| e.to_string())?;
        self.reload_defects();
        self.reload_video_tag_map()?;
        if let Some(id) = self.current_video {
            if !self.videos.iter().any(|v| v.id == id) {
                self.current_video = self.videos.first().map(|v| v.id);
            }
        } else {
            self.current_video = self.videos.first().map(|v| v.id);
        }
        if let Some(id) = self.current_video {
            self.select_video(id);
        }
        Ok(())
    }

    fn reload_video_tag_map(&mut self) -> Result<(), String> {
        self.video_tag_map.clear();
        if self.data_source == DataSource::Remote {
            for video in &self.videos {
                if let Some(remote_item_id) = self.remote_item_ids.get(&video.id) {
                    if let Some(item) = self
                        .remote_items
                        .iter()
                        .find(|item| item.item_id == *remote_item_id)
                    {
                        let ids = self.tag_ids_for_remote_names(&item.tags);
                        if !ids.is_empty() {
                            self.video_tag_map.insert(video.id, ids);
                        }
                    }
                }
            }
            return Ok(());
        }
        for video in &self.videos {
            if let Ok(ids) = self.service.get_video_tag_ids(video.id) {
                if !ids.is_empty() {
                    self.video_tag_map.insert(video.id, ids);
                }
            }
        }
        Ok(())
    }

    fn reload_tags(&mut self) -> Result<(), String> {
        self.all_tags = self.service.list_tags().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reload_markers(&mut self) {
        if self.data_source == DataSource::Remote {
            self.markers.clear();
            return;
        }
        if let Some(id) = self.current_video {
            self.markers = self.service.list_markers(id).unwrap_or_default();
        }
    }

    fn reload_segments(&mut self) {
        if self.data_source == DataSource::Remote {
            self.segments.clear();
            return;
        }
        if let Some(id) = self.current_video {
            self.segments = self.service.list_segments(id).unwrap_or_default();
        }
    }

    fn recent_tasks_ui(&mut self, ui: &mut egui::Ui) {
        let recent: Vec<_> = self
            .action_history
            .iter()
            .filter(|entry| entry.module == "视频评审")
            .take(5)
            .cloned()
            .collect();
        widgets::grouped_section(ui, "最近任务", |ui| {
            if recent.is_empty() {
                ui.label(RichText::new("暂无任务记录").small().weak());
                return;
            }
            for entry in recent {
                ui.label(
                    RichText::new(format!(
                        "{} · {} · 成功 {} / 失败 {}",
                        entry.operation,
                        entry.status.label(),
                        entry.success_count,
                        entry.failure_count
                    ))
                    .size(11.0),
                );
                ui.label(RichText::new(entry.target).small().weak());
                if let Some(detail) = entry.detail {
                    ui.label(
                        RichText::new(detail.lines().next().unwrap_or_default())
                            .small()
                            .weak(),
                    );
                }
                ui.add_space(4.0);
            }
        });
    }

    fn video_export_controls_ui(&mut self, ui: &mut egui::Ui, schema: &VideoExportSchema) {
        ui.horizontal_wrapped(|ui| {
            if widgets::compact_secondary_button(ui, "全选列", true).clicked() {
                self.video_export_column_keys =
                    schema.columns.iter().map(|c| c.key.clone()).collect();
                self.video_export_columns_initialized = true;
            }
            if widgets::compact_secondary_button(ui, "清空列", true).clicked() {
                self.video_export_column_keys.clear();
                self.video_export_columns_initialized = true;
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.video_export_template_name)
                    .hint_text("模板名")
                    .desired_width(110.0),
            );
            if widgets::compact_primary_button(
                ui,
                "保存模板",
                schema.columns.iter().any(|c| c.enabled),
            )
            .clicked()
            {
                self.save_video_export_template();
            }
        });
        let templates = GuiPrefs::load().export_templates_for("视频评审");
        if !templates.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("模板").small().weak());
                for template in templates.iter().take(4) {
                    if widgets::compact_secondary_button(ui, &template.name, true).clicked() {
                        self.video_export_column_keys = template.columns.clone();
                        self.video_export_columns_initialized = true;
                        self.video_export_template_name = template.name.clone();
                    }
                }
            });
        }
        ui.collapsing("导出列", |ui| {
            ui.horizontal_wrapped(|ui| {
                for column in schema.columns.clone() {
                    let mut on = self.video_export_column_keys.contains(&column.key);
                    if ui.checkbox(&mut on, &column.label).changed() {
                        self.video_export_columns_initialized = true;
                        if on {
                            if !self.video_export_column_keys.contains(&column.key) {
                                self.video_export_column_keys.push(column.key);
                            }
                        } else {
                            self.video_export_column_keys
                                .retain(|key| key != &column.key);
                        }
                    }
                }
            });
        });
    }

    fn video_export_schema(&mut self) -> VideoExportSchema {
        let base = VideoExportSchema::default();
        let all_keys: Vec<String> = base.columns.iter().map(|c| c.key.clone()).collect();
        if !self.video_export_columns_initialized {
            self.video_export_column_keys = all_keys.clone();
            self.video_export_columns_initialized = true;
        } else {
            self.video_export_column_keys
                .retain(|key| all_keys.contains(key));
        }
        base.with_enabled_keys(&self.video_export_column_keys)
    }

    fn video_export_request(&mut self, batch_id: i64, dest: PathBuf) -> VideoExportRequest {
        let schema = self.video_export_schema();
        let mut request = VideoExportRequest::new(batch_id, dest);
        request.schema = schema;
        if !self.selected_ids.is_empty() {
            request = request.selected(self.selected_ids.clone());
        }
        request
    }

    fn save_video_export_template(&mut self) {
        let name = if self.video_export_template_name.trim().is_empty() {
            "默认导出".to_string()
        } else {
            self.video_export_template_name.trim().to_string()
        };
        let mut prefs = GuiPrefs::load();
        prefs.upsert_export_template(ExportTemplate {
            module: "视频评审".into(),
            name: name.clone(),
            columns: self.video_export_column_keys.clone(),
        });
        let _ = prefs.save();
        self.video_export_template_name = name.clone();
        self.status_hint = format!("已保存导出模板「{name}」");
    }

    fn record_batch_action(
        &mut self,
        operation: impl Into<String>,
        target: impl Into<String>,
        result: &BatchOperationResult,
        elapsed_ms: u64,
    ) {
        let status = if result.failed == 0 {
            ActionHistoryStatus::Succeeded
        } else if result.applied > 0 {
            ActionHistoryStatus::PartiallyFailed
        } else {
            ActionHistoryStatus::Failed
        };
        self.record_action(
            operation,
            target,
            status,
            result.applied,
            result.failed,
            result.requested,
            elapsed_ms,
            (!result.failures.is_empty()).then(|| result.failures.join("\n")),
        );
    }

    fn record_action(
        &mut self,
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
            module: "视频评审".into(),
            operation: operation.into(),
            target: target.into(),
            status,
            success_count,
            failure_count,
            total_count,
            elapsed_ms,
            detail,
        };
        let mut prefs = GuiPrefs::load();
        prefs.push_action_history(entry);
        self.action_history = prefs.action_history.clone();
        let _ = prefs.save();
    }

    fn load_thumb(&mut self, ctx: &Context, path: &PathBuf) -> Option<TextureHandle> {
        crate::video_review::ui::video_list::load_thumb_texture(ctx, &mut self.thumb_textures, path)
    }

    fn poll_errors(&mut self) {
        if let Some(err) = self.error.take() {
            self.status_hint = err;
        }
        if !self.status_hint.is_empty() && self.output.status_message.is_empty() {
            self.output.status_message = self.status_hint.clone();
        }
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
        self.remote_item_ids.clear();
        self.pending_open_remote_batch_id = None;
        self.status_hint = reason.into();
        let _ = self.reload_batches();
    }

    fn start_remote_batches_fetch(&mut self) {
        if !crate::remote::remote_enabled(&self.remote_config) {
            self.switch_to_local("远程未配置，已回退本地");
            return;
        }
        let cfg = self.remote_config.clone();
        self.remote_loading = true;
        self.batches_fetch = Some(RemoteFetch::spawn(move || {
            let _ = crate::remote::probe_remote_health(&cfg)?;
            crate::remote::list_remote_review_batches(&cfg, RemoteBatchKind::Video)
        }));
    }

    fn start_remote_items_fetch(&mut self, remote_batch_id: String) {
        let cfg = self.remote_config.clone();
        self.remote_batch_id = Some(remote_batch_id.clone());
        self.remote_loading = true;
        self.items_fetch = Some(RemoteFetch::spawn(move || {
            crate::remote::fetch_batch_items_with_thumbs(&cfg, &remote_batch_id)
        }));
    }

    fn poll_remote_fetches(&mut self, ctx: &Context) {
        let batches_result = self.batches_fetch.as_ref().and_then(|fetch| fetch.poll());
        if let Some(result) = batches_result {
            self.batches_fetch = None;
            self.remote_loading = self.items_fetch.is_some() || self.asset_fetch.is_some();
            match result {
                Ok(summaries) => {
                    self.apply_remote_batches(summaries);
                    self.status_hint = format!("已加载 {} 个远程视频批次", self.batches.len());
                }
                Err(e) => self.switch_to_local(format!("远程视频批次加载失败，已回退本地：{e}")),
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
                    self.status_hint = format!("远程视频条目 {} 个", self.videos.len());
                }
                Err(e) => self.switch_to_local(format!("远程视频条目加载失败，已回退本地：{e}")),
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
                Ok((video_id, path)) => {
                    if let Some(video) = self.videos.iter_mut().find(|video| video.id == video_id) {
                        video.file_path = path;
                    }
                    if self.current_video == Some(video_id) {
                        self.timeline_thumbs.clear();
                    }
                    self.status_hint = "原视频已下载到本地缓存".into();
                }
                Err(e) => self.status_hint = format!("原视频下载失败：{e}"),
            }
            ctx.request_repaint();
        } else if self.asset_fetch.is_some() {
            ctx.request_repaint();
        }
    }

    fn apply_remote_batches(&mut self, summaries: Vec<RemoteReviewBatchSummary>) {
        self.remote_id_map.clear();
        self.remote_batches = summaries;
        self.batches = self
            .remote_batches
            .iter()
            .map(|summary| {
                let b = crate::remote::batch_from_summary(&mut self.remote_id_map, summary);
                VideoBatch {
                    id: b.id,
                    name: b.name,
                    total_count: b.total_count,
                    created_at: b.created_at,
                    updated_at: b.updated_at,
                }
            })
            .collect();

        let previous_batch = self.current_batch;
        let preferred = self
            .pending_open_remote_batch_id
            .take()
            .and_then(|rid| self.remote_id_map.local_of(&rid));
        if let Some(local) = preferred {
            self.current_batch = Some(local);
        } else if let Some(cur) = self.current_batch {
            if !self.batches.iter().any(|batch| batch.id == cur) {
                self.current_batch = self.batches.first().map(|batch| batch.id);
            }
        } else {
            self.current_batch = self.batches.first().map(|batch| batch.id);
        }

        if self.current_batch != previous_batch {
            self.current_video = None;
            self.videos.clear();
            self.remote_items.clear();
            self.remote_item_ids.clear();
            self.selected_ids.clear();
            self.video_tag_map.clear();
        }

        if let Some(batch_id) = self.current_batch {
            if let Some(remote_batch_id) =
                self.remote_id_map.remote_of(batch_id).map(str::to_string)
            {
                self.remote_batch_id = Some(remote_batch_id.clone());
                self.start_remote_items_fetch(remote_batch_id);
            }
        } else {
            self.remote_batch_id = None;
            self.videos.clear();
            self.remote_items.clear();
            self.remote_item_ids.clear();
        }
    }

    fn apply_remote_items(&mut self, pairs: Vec<(RemoteReviewItem, Option<PathBuf>)>) {
        let Some(batch_local) = self.current_batch else {
            return;
        };
        if let Some(remote_batch_id) = self.remote_id_map.remote_of(batch_local) {
            self.remote_batch_id = Some(remote_batch_id.to_string());
        }

        self.remote_items = pairs.iter().map(|(item, _)| item.clone()).collect();
        let mut videos: Vec<VideoItem> = pairs
            .iter()
            .map(|(item, thumb)| self.video_from_remote_item(batch_local, item, thumb.clone()))
            .collect();
        self.video_list_state.filter.apply_in_memory(&mut videos);
        self.videos = videos;

        self.remote_item_ids.clear();
        for item in &self.remote_items {
            if let Some(local) = self.remote_id_map.local_of(&item.item_id) {
                self.remote_item_ids.insert(local, item.item_id.clone());
            }
        }
        let _ = self.reload_video_tag_map();
        self.selected_ids
            .retain(|id| self.videos.iter().any(|video| video.id == *id));
        if let Some(cur) = self.current_video {
            if !self.videos.iter().any(|video| video.id == cur) {
                self.current_video = self.videos.first().map(|video| video.id);
            }
        } else {
            self.current_video = self.videos.first().map(|video| video.id);
        }
        if let Some(id) = self.current_video {
            self.select_video(id);
        } else {
            self.remark_buf.clear();
            self.offset_buf.clear();
            self.device_model_buf.clear();
            self.current_tag_ids.clear();
            self.markers.clear();
            self.segments.clear();
        }
    }

    fn video_from_remote_item(
        &mut self,
        batch_local_id: i64,
        item: &RemoteReviewItem,
        thumb_path: Option<PathBuf>,
    ) -> VideoItem {
        let id = self.remote_id_map.intern(&item.item_id);
        let ts = chrono::Utc
            .timestamp_opt(item.updated_at as i64, 0)
            .single()
            .unwrap_or_else(chrono::Utc::now);
        let file_path = thumb_path
            .clone()
            .unwrap_or_else(|| crate::remote::placeholder_path_for_asset(&item.asset));
        VideoItem {
            id,
            batch_id: batch_local_id,
            file_path,
            status: crate::remote::remote_status_to_local(item.status),
            remark: (!item.remark.is_empty()).then(|| item.remark.clone()),
            thumbnail_path: thumb_path,
            duration_ms: item.duration_ms.unwrap_or(0),
            fps: 0.0,
            width: item.width.unwrap_or(0),
            height: item.height.unwrap_or(0),
            video_codec: item.asset.mime.clone().unwrap_or_else(|| "remote".into()),
            audio_codec: None,
            bitrate_kbps: None,
            device_model: None,
            offset_ms: 0,
            created_at: ts,
            updated_at: ts,
            deleted_at: None,
        }
    }

    fn ensure_remote_original_downloaded(&mut self, video_id: i64) {
        if self.data_source != DataSource::Remote {
            return;
        }
        if self
            .asset_fetch
            .as_ref()
            .map(|fetch| fetch.is_pending())
            .unwrap_or(false)
        {
            return;
        }
        let Some(remote_item_id) = self.remote_item_ids.get(&video_id).cloned() else {
            return;
        };
        let Some(remote_item) = self
            .remote_items
            .iter()
            .find(|item| item.item_id == remote_item_id)
            .cloned()
        else {
            return;
        };
        if let Some(video) = self.videos.iter().find(|video| video.id == video_id) {
            let path = &video.file_path;
            let is_thumb = video.thumbnail_path.as_ref() == Some(path);
            if path.exists() && !path.to_string_lossy().starts_with("remote://") && !is_thumb {
                return;
            }
        }
        let cfg = self.remote_config.clone();
        let asset = remote_item.asset.clone();
        self.remote_loading = true;
        self.status_hint = "正在下载远程原视频…".into();
        self.asset_fetch = Some(RemoteFetch::spawn(move || {
            let path = crate::remote::ensure_remote_asset_local(&cfg, &asset)?;
            Ok((video_id, path))
        }));
    }

    fn remote_video_stats(&self) -> crate::video_review::domain::BatchStats {
        let mut stats = crate::video_review::domain::BatchStats::default();
        for item in &self.remote_items {
            stats.increment(crate::remote::remote_status_to_local(item.status));
        }
        stats
    }

    fn sync_remote_video_batch(&mut self, name: &str, paths: &[PathBuf]) -> Option<String> {
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
            crate::remote::RemoteBatchKind::Video,
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
                        return Some(format!(
                            "远程视频批次 {}（读取条目失败：{e}）",
                            batch.batch_id
                        ));
                    }
                };
                self.remember_remote_video_items(&items);
                let inputs = items.iter().map(|item| item.asset.clone()).collect();
                let extras = vec![
                    ("batch_id".into(), batch.batch_id.clone()),
                    ("batch_name".into(), name.to_string()),
                    ("probe".into(), "true".into()),
                    ("extract_cover".into(), "true".into()),
                    ("contact_sheet".into(), "true".into()),
                ];
                match crate::remote::submit_module_job(
                    &self.remote_config,
                    crate::remote::RemoteJobSource::VideoReview,
                    inputs,
                    extras,
                ) {
                    Ok((status, _)) => Some(format!(
                        "远程视频批次 {} · 任务 {}",
                        batch.batch_id, status.job_id
                    )),
                    Err(e) => Some(format!(
                        "远程视频批次 {}（任务提交失败：{e}）",
                        batch.batch_id
                    )),
                }
            }
            Err(e) => {
                self.remote_batch_id = None;
                self.remote_item_ids.clear();
                Some(format!("远程视频批次创建失败：{e}"))
            }
        }
    }

    fn remember_remote_video_items(&mut self, items: &[crate::remote::RemoteReviewItem]) {
        self.remote_item_ids.clear();
        let mut used = std::collections::HashSet::new();
        for video in &self.videos {
            let Some(local_name) = video.file_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Some(item) = items
                .iter()
                .find(|item| !used.contains(&item.item_id) && item.asset.name == local_name)
            {
                used.insert(item.item_id.clone());
                self.remote_item_ids.insert(video.id, item.item_id.clone());
            }
        }
    }

    fn sync_remote_video_statuses(&mut self, ids: &[i64], status: ReviewStatus) -> usize {
        ids.iter()
            .filter(|id| self.set_remote_video_status(**id, status).is_some())
            .count()
    }

    fn sync_remote_video_tags(&mut self, ids: &[i64], tag_ids: &[i64]) -> usize {
        let names = self.remote_video_tag_names(tag_ids);
        let local_tag_ids = tag_ids.to_vec();
        ids.iter()
            .filter(|id| {
                self.set_remote_video_tags(**id, local_tag_ids.clone(), names.clone())
                    .is_some()
            })
            .count()
    }

    fn sync_remote_video_item(
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

    fn set_remote_video_status(&mut self, id: i64, status: ReviewStatus) -> Option<String> {
        if let Some(video) = self.videos.iter_mut().find(|video| video.id == id) {
            video.status = status;
            video.updated_at = chrono::Utc::now();
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
        self.sync_remote_video_item(
            id,
            Some(crate::remote::local_status_to_remote(status)),
            None,
            None,
        )
    }

    fn set_remote_video_remark(&mut self, id: i64, remark: String) -> Option<String> {
        if let Some(video) = self.videos.iter_mut().find(|video| video.id == id) {
            video.remark = (!remark.is_empty()).then(|| remark.clone());
            video.updated_at = chrono::Utc::now();
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
        self.sync_remote_video_item(id, None, Some(remark), None)
    }

    fn set_remote_video_tags(
        &mut self,
        id: i64,
        tag_ids: Vec<i64>,
        tag_names: Vec<String>,
    ) -> Option<String> {
        if tag_ids.is_empty() {
            self.video_tag_map.remove(&id);
        } else {
            self.video_tag_map.insert(id, tag_ids.clone());
        }
        if self.current_video == Some(id) {
            self.current_tag_ids = tag_ids;
        }
        if let Some(remote_item_id) = self.remote_item_ids.get(&id).cloned() {
            if let Some(item) = self
                .remote_items
                .iter_mut()
                .find(|item| item.item_id == remote_item_id)
            {
                item.tags = tag_names.clone();
                item.updated_at = chrono::Utc::now().timestamp().max(0) as u64;
            }
        }
        self.sync_remote_video_item(id, None, None, Some(tag_names))
    }

    fn remote_video_tag_names(&self, tag_ids: &[i64]) -> Vec<String> {
        tag_ids
            .iter()
            .filter_map(|id| {
                self.all_tags
                    .iter()
                    .find(|tag| tag.id == *id)
                    .map(|tag| tag.name.clone())
            })
            .collect()
    }

    fn tag_ids_for_remote_names(&self, names: &[String]) -> Vec<i64> {
        self.all_tags
            .iter()
            .filter(|tag| names.iter().any(|name| name == &tag.name))
            .map(|tag| tag.id)
            .collect()
    }
}

fn format_video_contact_sheets(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        "无".into()
    } else if paths.len() == 1 {
        paths[0].display().to_string()
    } else {
        format!("{} 页", paths.len())
    }
}

fn video_paths_for_folder(folder: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in jwalk::WalkDir::new(folder)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(crate::video_review::domain::is_video_extension)
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn fixed_grouped_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    outer_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    widgets::section_header(ui, title);
    ui.add_space(6.0);
    fixed_grouped_frame(ui, outer_width, add_contents)
}

fn fixed_grouped_frame<R>(
    ui: &mut egui::Ui,
    outer_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let dark = ui.style().visuals.dark_mode;
    let inner_width = (outer_width - 32.0).max(120.0);
    Frame::new()
        .fill(theme::grouped_fill(dark))
        .corner_radius(CornerRadius::same(theme::GROUP_RADIUS))
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_min_width(inner_width);
            ui.set_max_width(inner_width);
            add_contents(ui)
        })
        .inner
}
