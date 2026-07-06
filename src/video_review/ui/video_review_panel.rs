//! 视频评审主面板。

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Context, RichText, ScrollArea, TextureHandle};

use crate::gui::widgets;
use crate::review::domain::image_item::ReviewStatus;
use crate::review::ui::status_buttons;
use crate::video_review::domain::{
  MarkerKind, VideoBatch, VideoItem, VideoMarker, VideoSegment, VideoTag,
};
use crate::video_review::service::{
  compute_layout, compute_quality_cell_size, grid_dimensions, max_export_duration_ms,
  GridVideoExportQuality, VideoExportRequest, VideoExportService, VideoReviewService,
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
    self.show_ffmpeg_banner(ui);

    let avail = ui.available_size();
    const LEFT_W: f32 = 260.0;
    const COL_GAP: f32 = 8.0;
    let main_w = (avail.x - LEFT_W - COL_GAP).max(180.0);

    ui.horizontal_top(|ui| {
      ui.vertical(|ui| {
        ui.set_width(LEFT_W);
        self.left_sidebar_ui(ctx, ui);
      });
      ui.add_space(COL_GAP);
      ui.vertical(|ui| {
        ui.set_width(main_w);
        self.center_ui(ctx, ui, egui::vec2(main_w, avail.y - 8.0));
      });
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
          match self.service.import_folder(&folder, None) {
            Ok(id) => {
              self.current_batch = Some(id);
              let _ = self.reload_batches();
              self.status_hint = format!("已导入批次：{}", folder.display());
            }
            Err(e) => self.error = Some(e.to_string()),
          }
        }
      }
      ScrollArea::vertical()
        .id_salt("video_review_batch_list")
        .max_height(120.0)
        .show(ui, |ui| {
        for batch in &self.batches.clone() {
          let selected = self.current_batch == Some(batch.id);
          if ui
            .selectable_label(selected, &batch.name)
            .clicked()
          {
            self.current_batch = Some(batch.id);
            let _ = self.reload_videos();
          }
        }
      });
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
        if self.selected_ids.len() < MAX_COMPARE_VIDEOS && !self.selected_ids.contains(&id) {
          self.selected_ids.push(id);
        }
      } else {
        self.selected_ids.retain(|x| *x != id);
      }
    }
  }

  fn center_ui(&mut self, ctx: &Context, ui: &mut egui::Ui, area: egui::Vec2) {
    self.attribute_panel_ui(ui);
    ui.add_space(8.0);

    widgets::grouped_section(ui, if self.compare_mode { "多视频对比" } else { "时间轴" }, |ui| {
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
          if widgets::compact_primary_button(ui, "导出宫格", can_export).clicked() {
            self.export_contact_sheet();
          }
          if widgets::compact_secondary_button(ui, "导出视频", can_export && ffmpeg_ok).clicked() {
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
      if self.compare_mode && self.selected_ids.len() >= 2 {
        self.compare.ui(ctx, ui, &self.service, &self.videos, egui::vec2(area.x, view_h.max(120.0)));
      } else if let Some(video) = self.current_video_item().cloned() {
        let mut compare = MultiVideoCompare::with_time(self.compare.current_time_ms);
        compare.ui(
          ctx,
          ui,
          &self.service,
          std::slice::from_ref(&video),
          egui::vec2(area.x, view_h.max(120.0)),
        );
        self.compare.current_time_ms = compare.current_time_ms;
      } else {
        ui.centered_and_justified(|ui| {
          ui.label("选择或导入视频开始评审");
        });
      }
    });
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

  fn attribute_panel_ui(&mut self, ui: &mut egui::Ui) {
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
    widgets::grouped_section_frame(ui, |ui| {
      ScrollArea::vertical()
        .id_salt("video_review_attr_panel")
        .max_height(ATTR_PANEL_MAX_H)
        .show(ui, |ui| match self.right_tab {
          RightTab::Review => self.review_tab_ui(ui),
          RightTab::Info => self.info_tab_ui(ui),
          RightTab::Markers => self.markers_tab_ui(ui),
          RightTab::Tags => self.tags_tab_ui(ui),
          RightTab::Export => self.export_tab_ui(ui),
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
        if self.service.batch_update_status(&ids, s).is_ok() {
          let _ = self.reload_videos();
          self.status_hint = format!("已批量更新 {} 个视频状态", ids.len());
        }
      }
      ui.label("批量备注追加");
      ui.text_edit_multiline(&mut self.batch_remark_buf);
      if widgets::compact_secondary_button(ui, "追加到选中", !self.batch_remark_buf.trim().is_empty())
        .clicked()
      {
        let ids = self.selected_ids.clone();
        let text = self.batch_remark_buf.clone();
        if self.service.batch_append_remark(&ids, &text).is_ok() {
          self.batch_remark_buf.clear();
          let _ = self.reload_videos();
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
        if widgets::compact_secondary_button(ui, "应用到选中", !self.batch_tag_ids.is_empty()).clicked()
        {
          let ids = self.selected_ids.clone();
          let tags = self.batch_tag_ids.clone();
          if self.service.batch_set_tags(&ids, &tags).is_ok() {
            self.status_hint = format!("已为 {} 个视频应用标签", ids.len());
          }
        }
      }
    }
  }

  fn info_tab_ui(&mut self, ui: &mut egui::Ui) {
    let Some(video) = self.current_video_item() else {
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
  }

  fn markers_tab_ui(&mut self, ui: &mut egui::Ui) {
    let Some(video_id) = self.current_video else {
      ui.label("未选择视频");
      return;
    };
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
        let jump_label = format!(
          "[{} - {}]",
          format_ms(seg.start_ms),
          format_ms(seg.end_ms)
        );
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
      if widgets::compact_primary_button(ui, "新建", !self.new_tag_name.trim().is_empty()).clicked() {
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
        ui.colored_label(
          Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
          "■",
        );
        if ui.checkbox(&mut on, &tag.name).changed() {
          if on {
            if !self.current_tag_ids.contains(&tag.id) {
              self.current_tag_ids.push(tag.id);
            }
          } else {
            self.current_tag_ids.retain(|id| *id != tag.id);
          }
          let _ = self
            .service
            .set_video_tags(video_id, &self.current_tag_ids);
        }
      });
    }
  }

  fn export_tab_ui(&mut self, ui: &mut egui::Ui) {
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
    if widgets::compact_primary_button(ui, "导出当前对比宫格…", can_export).clicked() {
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
      ui.label(format!(
        "布局 {}×{} · 输出 {}×{} px（单格 {}×{}，源分辨率）· 从 {} 起",
        rows,
        cols,
        cols as u32 * cell_w,
        rows as u32 * cell_h,
        cell_w,
        cell_h,
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
    if widgets::compact_primary_button(ui, "导出对比拼接视频…", can_export_video).clicked() {
      self.export_compare_grid_video();
    }

    ui.separator();
    if widgets::compact_secondary_button(ui, "导出 CSV…", true).clicked() {
      if let Some(path) = rfd::FileDialog::new()
        .set_file_name("video_review.csv")
        .save_file()
      {
        match VideoExportService::export_csv(
          self.service.repo(),
          &VideoExportRequest {
            batch_id,
            dest: path,
          },
        ) {
          Ok(r) => {
            self.output.status_message =
              format!("已导出 CSV（{} 行）→ {}", r.row_count, r.dest.display());
          }
          Err(e) => self.error = Some(e.to_string()),
        }
      }
    }
    if widgets::compact_secondary_button(ui, "导出 JSON…", true).clicked() {
      if let Some(path) = rfd::FileDialog::new()
        .set_file_name("video_review.json")
        .save_file()
      {
        match VideoExportService::export_json(self.service.repo(), batch_id, &path) {
          Ok(()) => {
            self.output.status_message = format!("已导出 JSON → {}", path.display());
          }
          Err(e) => self.error = Some(e.to_string()),
        }
      }
    }

    ui.add_space(8.0);
    if let Ok(stats) = self.service.frame_cache_stats() {
      ui.label(format!(
        "抽帧缓存：{} 个文件，{:.1} MB",
        stats.file_count,
        stats.total_bytes as f64 / 1_048_576.0
      ));
    }
    if widgets::compact_secondary_button(ui, "清理抽帧缓存", true).clicked() {
      match self.service.clear_frame_cache() {
        Ok(n) => self.output.status_message = format!("已清理 {n} 个缓存文件"),
        Err(e) => self.error = Some(e.to_string()),
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
        }
        Err(e) => {
          let msg = e.to_string();
          if msg.contains("ffmpeg") || msg.contains("抽帧") {
            self.error = Some(format!("抽帧失败：{msg}"));
          } else if msg.contains("save") || msg.contains("写入") || msg.contains("permission") {
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
      self.error = Some("ffmpeg 不可用，无法导出拼接视频。请安装 ffmpeg 并加入 PATH 后重启。".into());
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
        }
        Err(e) => {
          let msg = e.to_string();
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
      self.timeline_thumbs.clear();
      if let Ok(ids) = self.service.get_video_tag_ids(id) {
        self.current_tag_ids = ids;
      }
      self.reload_markers();
      self.reload_segments();
    }
  }

  fn current_video_item(&self) -> Option<&VideoItem> {
    self
      .current_video
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
