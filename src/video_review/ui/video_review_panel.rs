//! 视频评审主面板。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{
    self, Color32, Context, CornerRadius, Frame, Margin, RichText, ScrollArea, TextureHandle,
};

use crate::gui::prefs::{self, ActionHistoryEntry, ActionHistoryStatus, ExportTemplate, GuiPrefs};
use crate::gui::{theme, widgets, BackgroundJob};
use crate::review::domain::image_item::ReviewStatus;
use crate::review::ui::status_buttons;
use crate::video_review::domain::{
    MarkerKind, VideoBatch, VideoItem, VideoMarker, VideoSegment, VideoTag,
};
use crate::video_review::service::{
    compute_layout, compute_quality_cell_size, grid_dimensions, max_export_duration_ms,
    BatchOperationResult, BatchScreenshotRequest, BatchScreenshotResult, BatchScreenshotService,
    GridVideoCaptionMode, GridVideoExportQuality, ScreenshotFormat, ScreenshotMode,
    VideoAnalysisService, VideoExportRequest, VideoExportSchema, VideoExportService,
    VideoReviewService, DEFAULT_INTERVAL_SECS, DEFAULT_MAX_SHOTS,
};
use crate::video_review::service::ffmpeg_backend::FfmpegBackend;
use crate::video_review::service::frame_cache::FrameCache;
use crate::video_review::service::screenshot_service::plan_shots;
use crate::ui::progress::ProgressReporter;
use crate::video_review::ui::hover_preview::HoverPreviewController;
use crate::video_review::ui::multi_compare::{format_ms, MultiVideoCompare, MAX_COMPARE_VIDEOS};
use crate::video_review::ui::video_list::{
    video_list_body_ui, video_list_toolbar_ui, VideoListAction, VideoListState,
};

#[derive(Debug, Clone, Default)]
pub struct VideoReviewPanelOutput {
    pub status_message: String,
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
    action_history: Vec<ActionHistoryEntry>,
    video_export_column_keys: Vec<String>,
    video_export_columns_initialized: bool,
    video_export_template_name: String,
    batch_remark_buf: String,
    batch_tag_ids: Vec<i64>,
    pending_delete_marker: Option<i64>,
    pending_delete_segment: Option<i64>,
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

impl VideoReviewPanel {
    pub fn new() -> Result<Self, String> {
        let service = VideoReviewService::open().map_err(|e| e.to_string())?;
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
            action_history: GuiPrefs::load().action_history,
            video_export_column_keys: Vec::new(),
            video_export_columns_initialized: false,
            video_export_template_name: String::from("默认导出"),
            batch_remark_buf: String::new(),
            batch_tag_ids: Vec::new(),
            pending_delete_marker: None,
            pending_delete_segment: None,
        };
        panel.reload_batches().map_err(|e| e.to_string())?;
        Ok(panel)
    }

    pub fn take_output(&mut self) -> VideoReviewPanelOutput {
        std::mem::take(&mut self.output)
    }

    pub fn ui(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        self.poll_errors();
        self.poll_screenshot_job(ctx);
        self.show_ffmpeg_banner(ui);

        const LEFT_W: f32 = 260.0;
        const COL_GAP: f32 = 8.0;
        const RIGHT_INSET: f32 = 28.0;
        let avail = ui.available_size();
        let main_w = (avail.x - LEFT_W - COL_GAP - RIGHT_INSET).max(180.0);

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(LEFT_W, avail.y),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    self.left_sidebar_ui(ctx, ui);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(COL_GAP, avail.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |_ui| {},
            );
            ui.allocate_ui_with_layout(
                egui::vec2(main_w, avail.y),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_max_width(main_w);
                    self.center_ui(ctx, ui, egui::vec2(main_w, avail.y - 8.0));
                },
            );
            ui.allocate_exact_size(egui::vec2(RIGHT_INSET, avail.y), egui::Sense::hover());
        });
    }

    fn show_ffmpeg_banner(&self, ui: &mut egui::Ui) {
        let avail = self.service.availability();
        if avail.ffmpeg_ok && avail.ffprobe_ok {
            return;
        }
        ui.horizontal(|ui| {
            ui.colored_label(
                Color32::from_rgb(255, 149, 0),
                "⚠ ffmpeg/ffprobe 未检测到，请安装并加入 PATH 后重启。视频导入与抽帧功能不可用。",
            );
        });
        ui.add_space(4.0);
    }

    fn left_sidebar_ui(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        widgets::grouped_section(ui, "批次", |ui| {
            if widgets::compact_primary_button(ui, "导入视频文件夹…", true).clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let started = Instant::now();
                    match self.service.import_folder(&folder, None) {
                        Ok(id) => {
                            self.current_batch = Some(id);
                            let _ = self.reload_batches();
                            self.status_hint = format!("已导入批次：{}", folder.display());
                            let total = self.videos.len();
                            self.record_action(
                                "导入视频文件夹",
                                folder.display().to_string(),
                                ActionHistoryStatus::Succeeded,
                                total,
                                0,
                                total,
                                started.elapsed().as_millis() as u64,
                                None,
                            );
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            self.record_action(
                                "导入视频文件夹",
                                folder.display().to_string(),
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
            ScrollArea::vertical()
                .id_salt("video_review_batch_list")
                .max_height(120.0)
                .show(ui, |ui| {
                    for batch in &self.batches.clone() {
                        let selected = self.current_batch == Some(batch.id);
                        if ui.selectable_label(selected, &batch.name).clicked() {
                            self.current_batch = Some(batch.id);
                            let _ = self.reload_videos();
                        }
                    }
                });
            if let Some(batch_id) = self.current_batch {
                if let Ok(stats) = self.service.batch_stats(batch_id) {
                    ui.label(
                        RichText::new(format!(
                            "统计：待评审 {} / 通过 {} / 待修正 {} / 驳回 {}",
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
                        if widgets::compact_primary_button(ui, "导出宫格", can_export).clicked()
                        {
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
                    self.compare.ui(
                        ctx,
                        ui,
                        &self.service,
                        &self.videos,
                        egui::vec2(pane_w, view_h.max(120.0)),
                    );
                } else if let Some(video) = self.current_video_item().cloned() {
                    let mut compare = MultiVideoCompare::with_time(self.compare.current_time_ms);
                    compare.ui(
                        ctx,
                        ui,
                        &self.service,
                        std::slice::from_ref(&video),
                        egui::vec2(pane_w, view_h.max(120.0)),
                    );
                    self.compare.current_time_ms = compare.current_time_ms;
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
            if self.service.update_status(video.id, s).is_ok() {
                let _ = self.reload_videos();
            }
        }
        ui.add_space(6.0);
        ui.label("备注");
        if ui.text_edit_multiline(&mut self.remark_buf).lost_focus() {
            let _ = self.service.update_remark(video.id, &self.remark_buf);
        }
        ui.add_space(6.0);
        ui.label("偏移校准 (ms)");
        ui.horizontal(|ui| {
            if ui.text_edit_singleline(&mut self.offset_buf).lost_focus() {
                if let Ok(v) = self.offset_buf.parse::<i64>() {
                    let _ = self.service.update_offset(video.id, v);
                    let _ = self.reload_videos();
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
                let result = self.service.batch_update_status_result(&ids, s);
                self.record_batch_action(
                    "批量更新状态",
                    format!("{} 个视频 → {}", ids.len(), s.label()),
                    &result,
                    started.elapsed().as_millis() as u64,
                );
                if result.is_success() {
                    let _ = self.reload_videos();
                    self.status_hint = format!("已批量更新 {} 个视频状态", result.applied);
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
                    let result = self.service.batch_set_tags_result(&ids, &tags);
                    self.record_batch_action(
                        "批量应用标签",
                        format!("{} 个视频 · {} 个标签", ids.len(), tags.len()),
                        &result,
                        started.elapsed().as_millis() as u64,
                    );
                    if result.is_success() {
                        self.status_hint = format!("已为 {} 个视频应用标签", result.applied);
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
            if let Err(e) = self.service.update_device_model(video.id, value) {
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
                    let _ = self.service.set_video_tags(video_id, &self.current_tag_ids);
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
            ui.colored_label(
                Color32::from_rgb(255, 149, 0),
                "ffmpeg 不可用，无法导出宫格图片或拼接视频。请安装 ffmpeg 并加入 PATH。",
            );
            ui.add_space(4.0);
        }

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
                Color32::from_rgb(255, 80, 80),
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
            ui.colored_label(Color32::from_rgb(52, 199, 89), format!("✓ {msg}"));
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
        let started = self.screenshot_job_started.take().unwrap_or_else(Instant::now);
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
        if let Ok(v) = self.service.get_video(id) {
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
            return Ok(());
        };
        self.videos = self
            .service
            .list_videos(batch_id, &self.video_list_state.filter)
            .map_err(|e| e.to_string())?;
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
        if let Some(id) = self.current_video {
            self.markers = self.service.list_markers(id).unwrap_or_default();
        }
    }

    fn reload_segments(&mut self) {
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
