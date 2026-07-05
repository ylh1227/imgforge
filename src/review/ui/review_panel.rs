//! 评审主面板：三栏布局，串联标注画布、对比视图、批量操作与转换队列联动。

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, ScrollArea};

use crate::gui::theme;
use crate::gui::widgets;
use crate::review::domain::annotation::Annotation;
use crate::review::domain::{
  BatchStats, ReviewBatch, ReviewImageItem, ReviewStatus,
};
use crate::review::error::ReviewResult;
use crate::review::service::{
  BatchAnnotateRequest, BatchRemarkRequest, BatchStatusRequest, ExportService,
  ShortcutAction, StatusTransitionWarning,
};
use crate::review::is_irreversible_transition;
use crate::review::RemarkWriteMode;
use crate::review::ui::annotation_canvas::AnnotationCanvasEvent;
use crate::review::ui::compare_view::{CompareDisplayMode, CompareView, MAX_MULTI_COMPARE_PANES};
use crate::review::ui::shortcuts::handle_shortcuts;
use crate::review::domain::image_item::next_image_id;
use crate::review::service::ReviewModuleConfig;
use crate::review::ui::properties_panel::{properties_panel_ui, PropertiesPanelState};
use crate::review::ui::shortcut_panel::{shortcut_panel_ui, ShortcutPanelState};
use crate::review::ui::sidebar::{
  batch_list_ui, format_stats, image_list_ui, status_buttons, SidebarState,
};
use crate::review::ui::ListThumbnailCache;
use crate::review::{ReviewConversionBridge, ReviewService};

/// 主应用向评审面板提供的上下文（评审模块不依赖 gui 内部实现）。
pub trait ReviewPanelHost {
  /// 格式转换页待处理/已导入的路径队列。
  fn conversion_queue_paths(&self) -> &[PathBuf];
  /// 转换输出目录（用于对比视图查找转换后预览）。
  fn output_directory(&self) -> &str;
}

/// 评审面板向主应用输出的联动指令。
#[derive(Debug, Clone, Default)]
pub struct ReviewPanelOutput {
  /// 将「通过」的图片路径加入格式转换队列。
  pub enqueue_approved: Vec<PathBuf>,
  /// 单图转换参数覆盖（评审标记带入队列），与 `enqueue_approved` 对应。
  pub enqueue_params: Vec<crate::review::ConversionTaskParams>,
  pub status_message: String,
  /// 请求主应用切回格式转换 Tab。
  pub switch_to_convert: bool,
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
  properties: PropertiesPanelState,
  shortcut_panel: ShortcutPanelState,
  show_shortcut_panel: bool,
  last_backup_minute: u64,
  right_tab: RightTab,
  all_tags: Vec<crate::review::ReviewTag>,
  current_image_tags: Vec<i64>,
  new_tag_name: String,
  new_tag_color_idx: usize,
  renaming_tag: Option<(i64, String)>,
  list_thumbs: ListThumbnailCache,
  /// 画布区域最近一次布局尺寸（用于「适应窗口」，避免用整窗 viewport 误算）。
  canvas_area_size: egui::Vec2,
  last_viewport_size: egui::Vec2,
  viewport_resize_frames: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RightTab {
  #[default]
  Review,
  Info,
  Annotations,
  Tags,
}

impl RightTab {
  fn label(self) -> &'static str {
    match self {
      Self::Review => "评审属性",
      Self::Info => "图片信息",
      Self::Annotations => "标注列表",
      Self::Tags => "标签",
    }
  }

  fn all() -> [Self; 4] {
    [Self::Review, Self::Info, Self::Annotations, Self::Tags]
  }
}

#[derive(Debug, Clone)]
enum DialogState {
  ConfirmBatchOp(BatchOpKind),
  IrreversibleStatus {
    target: ReviewStatus,
    warnings: Vec<StatusTransitionWarning>,
    confirm: bool,
  },
}

#[derive(Debug, Clone, Copy)]
enum BatchOpKind {
  SetStatus(ReviewStatus),
  ClearAnnotations,
  AddRemark,
  CopyCurrentAnnotations,
}

impl ReviewPanel {
  pub fn new() -> ReviewResult<Self> {
    let service = ReviewService::open()?;
    let shortcuts = service.shortcuts.clone();
    let config = ReviewModuleConfig::load().unwrap_or_default();
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
      properties: PropertiesPanelState::default(),
      shortcut_panel: ShortcutPanelState::new(&shortcuts),
      show_shortcut_panel: false,
      last_backup_minute: 0,
      right_tab: RightTab::default(),
      all_tags: Vec::new(),
      current_image_tags: Vec::new(),
      new_tag_name: String::new(),
      new_tag_color_idx: 0,
      renaming_tag: None,
      list_thumbs: ListThumbnailCache::default(),
      canvas_area_size: egui::Vec2::ZERO,
      last_viewport_size: egui::Vec2::ZERO,
      viewport_resize_frames: 0,
    };
    let _ = panel.reload_tags();
    panel.reload_batches()?;
    if let Ok((batch, image)) = panel.service.restore_session() {
      panel.current_batch = batch;
      panel.current_image = image;
      let _ = panel.reload_images();
      panel.load_current_annotations();
    }
    Ok(panel)
  }

  /// 由主应用在切换 Tab 前调度：从转换队列创建评审批次。
  pub fn schedule_import_from_queue(&mut self, paths: Vec<PathBuf>, batch_name: impl Into<String>) {
    if !paths.is_empty() {
      self.pending_import = Some((paths, batch_name.into()));
    }
  }

  pub fn take_output(&mut self) -> ReviewPanelOutput {
    std::mem::take(&mut self.output)
  }

  /// 渲染评审面板（三栏 + 顶栏 + 底栏）。
  pub fn ui(
    &mut self,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    host: &dyn ReviewPanelHost,
  ) {
    self.process_pending_import();
    self.handle_shortcut_actions(ctx);
    let vp = viewport_size(ctx);
    if (vp - self.last_viewport_size).length_sq() > 4.0 {
      self.viewport_resize_frames = 12;
      self.last_viewport_size = vp;
    } else if self.viewport_resize_frames > 0 {
      self.viewport_resize_frames -= 1;
    }
    self
      .compare_view
      .set_defer_texture_load(self.viewport_resize_frames > 0);
    // 切换对比模式下：空格键手动翻转原图/转换后
    if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
      self.compare_view.toggle_flip();
    }
    self.show_dialogs(ctx);
    self.maybe_scheduled_backup();

    let dark = ui.style().visuals.dark_mode;
    let narrow = ui.available_width() < theme::REVIEW_NARROW_BREAKPOINT;

    widgets::navigation_header(ui, "图片评审");
    ui.add_space(20.0);

    widgets::grouped_section(ui, "操作", |ui| {
      self.top_toolbar(ui, host);
    });

    ui.add_space(16.0);

    if let Some(err) = &self.error {
      widgets::error_banner(ui, err);
      ui.add_space(8.0);
    }

    widgets::status_banner(ui, &self.status_message(dark), false);
    ui.add_space(12.0);

    self.main_workflow_bar(ui, ctx, dark);
    ui.add_space(16.0);

    if self.show_shortcut_panel {
      widgets::grouped_section(ui, "快捷键", |ui| {
        if shortcut_panel_ui(ui, &mut self.shortcut_panel) {
          let _ = self.service.save_shortcuts(&self.shortcut_panel.draft);
          self.set_status("快捷键已更新");
        }
      });
      ui.add_space(16.0);
    }

    if narrow {
      self.layout_stacked(ui, ctx, host, dark);
    } else {
      self.layout_three_column(ui, ctx, host, dark);
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
        item
          .file_path
          .file_name()
          .and_then(|n| n.to_str())
          .unwrap_or("—")
      );
    }
    "选择评审批次与图片，或从转换队列导入".into()
  }

  fn top_toolbar(&mut self, ui: &mut egui::Ui, host: &dyn ReviewPanelHost) {
    ui.horizontal_wrapped(|ui| {
      if widgets::compact_primary_button(ui, "从文件夹创建", true).clicked() {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
          self.create_batch_from_folder(&folder);
        }
      }

      let queue_len = host.conversion_queue_paths().len();
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

      ui.separator();

      if widgets::compact_secondary_button(ui, "导出 CSV", self.current_batch.is_some()).clicked()
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

      ui.separator();

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
    let can_next = idx
      .map(|i| i + 1 < self.images.len())
      .unwrap_or(false);

    widgets::grouped_section(ui, "常用", |ui| {
      let selected_count = self.sidebar.selected_ids.len();
      let page_label = idx.map(|i| format!("{}/{}", i + 1, self.images.len()));
      let left_w = widgets::workflow_left_zone_width(ui, page_label.as_deref());

      widgets::toolbar_row(ui, |ui| {
        widgets::toolbar_left_zone(ui, left_w, |ui| {
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
        });
        widgets::toolbar_separator(ui);
        let current_status = self.current_item().map(|item| item.status);
        if let Some(status) = status_buttons(ui, current_status) {
          if let Some(id) = self.current_image {
            self.set_image_status(id, status);
          }
        }
      });

      ui.add_space(8.0);

      widgets::toolbar_row(ui, |ui| {
        widgets::toolbar_left_zone(ui, left_w, |ui| {
          widgets::toolbar_field_label(ui, "对比模式", dark);
          self.compare_view.mode_selector_ui(ui);
        });
        widgets::toolbar_separator(ui);
        let selected = selected_count;
        let batch_label = format!("批量对比 ({selected})");
        let batch_clicked = if selected >= 2 {
          widgets::compact_primary_button(ui, &batch_label, true).clicked()
        } else {
          widgets::compact_secondary_button(ui, &batch_label, false).clicked()
        };
        if batch_clicked {
          self.start_batch_compare();
        }
      });

      ui.add_space(8.0);

      widgets::toolbar_row(ui, |ui| {
        widgets::toolbar_left_zone(ui, left_w, |ui| {
          let canvas = self.canvas_size_for_view(ctx);
          if widgets::compact_secondary_button(ui, "适应窗口", has_image || selected_count >= 2)
            .clicked()
          {
            self.compare_view.fit_to_window(canvas);
          }
          if widgets::compact_secondary_button(ui, "100%", has_image || selected_count >= 2).clicked()
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
        });
        widgets::toolbar_separator(ui);
        if widgets::compact_secondary_button(ui, "仅显示未评审", self.current_batch.is_some())
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

        widgets::toolbar_separator(ui);

        let approved = self
          .current_batch
          .map(|batch_id| {
            self
              .batch_stats
              .iter()
              .find(|(id, _)| *id == batch_id)
              .map(|(_, s)| s.approved)
              .unwrap_or(0)
          })
          .unwrap_or(0);
        if widgets::compact_primary_button(
          ui,
          &format!("回流转换队列 ({approved})"),
          approved > 0,
        )
        .clicked()
        {
          self.enqueue_approved_to_convert();
        }
      });
    });
  }

  fn layout_three_column(
    &mut self,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    host: &dyn ReviewPanelHost,
    dark: bool,
  ) {
    ui.horizontal_top(|ui| {
      ui.vertical(|ui| {
        ui.set_width(260.0);
        self.left_column(ui, ctx, dark);
      });

      ui.separator();

      ui.vertical(|ui| {
        ui.set_min_width(320.0);
        self.center_column(ui, ctx, host);
      });

      ui.separator();

      ui.vertical(|ui| {
        ui.set_width(280.0);
        self.right_column(ui, dark);
      });
    });
  }

  fn layout_stacked(
    &mut self,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    host: &dyn ReviewPanelHost,
    dark: bool,
  ) {
    ui.vertical(|ui| {
      self.left_column(ui, ctx, dark);
      ui.separator();
      self.center_column(ui, ctx, host);
      ui.separator();
      self.right_column(ui, dark);
    });
  }

  fn left_column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, dark: bool) {
    widgets::grouped_section(ui, "批次", |ui| {
      ui.add(
        egui::TextEdit::singleline(&mut self.sidebar.batch_name_input)
          .hint_text("批次名称")
          .margin(egui::vec2(12.0, 10.0)),
      );
      ui.add_space(4.0);
      if let Some(id) = batch_list_ui(ui, &self.batches, &self.batch_stats, self.current_batch) {
        self.current_batch = Some(id);
        self.current_image = None;
        self.list_thumbs.clear();
        let _ = self.reload_images();
      }
    });

    ui.add_space(16.0);

    widgets::grouped_section(ui, "图片", |ui| {
      let list_action = image_list_ui(
        ui,
        ctx,
        &self.images,
        self.current_image,
        &mut self.sidebar,
        &mut self.list_thumbs,
      );
      if list_action.reload {
        let _ = self.reload_images();
      }
      if let Some(id) = list_action.selected {
        self.select_image(id);
      }
    });

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

  fn center_column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, host: &dyn ReviewPanelHost) {
    let dark = ui.style().visuals.dark_mode;
    let multi_active = self.compare_view.mode == CompareDisplayMode::MultiSplit;
    let section_title = if multi_active {
      format!(
        "画布 · 多图对比 ({})",
        self.sidebar.selected_ids.len()
      )
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
              RichText::new("请先在左侧勾选至少 2 张，再点常用栏左侧蓝色「批量对比」")
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
        let thumb_path = crate::review::service::ThumbnailService::valid_cache_path(&item.file_path);
        let thumb_ref = thumb_path.as_deref();
        self.update_converted_preview(host.output_directory(), &item.file_path);

        let events = {
          let mut events = self.compare_view.tools_ui(ui);
          events.extend(self.compare_view.ui(
            ui,
            ctx,
            &item.file_path,
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
    if ui
      .add(
        egui::TextEdit::multiline(&mut self.remark_buf)
          .margin(egui::vec2(12.0, 10.0))
          .desired_rows(4),
      )
      .changed()
    {
      if let Some(id) = self.current_image {
        let remark = self.remark_buf.clone();
        if let Err(e) = self.service.set_remark(id, &remark) {
          self.error = Some(e.to_string());
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
        RichText::new(format!("评审时间：{}", item.updated_at.format("%Y-%m-%d %H:%M")))
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
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
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
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(
              rect.center(),
              6.0,
              egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
            );

            if let Some((rid, buf)) = self.renaming_tag.as_mut() {
              if *rid == tag.id {
                let resp = ui.add(egui::TextEdit::singleline(buf).desired_width(110.0));
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                  rename_commit = Some((tag.id, buf.clone()));
                }
                if widgets::compact_secondary_button(ui, "确定", true).clicked() {
                  rename_commit = Some((tag.id, buf.clone()));
                }
                return;
              }
            }

            let mut on = self.current_image_tags.contains(&tag.id);
            if ui.add_enabled(has_image, egui::Checkbox::new(&mut on, &tag.name)).changed() {
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
    if let Some(id) = self.current_image {
      self.current_image_tags = self.service.tags_for_image(id).unwrap_or_default();
    } else {
      self.current_image_tags.clear();
    }
  }

  fn right_column(&mut self, ui: &mut egui::Ui, dark: bool) {
    // 顶部 Tab 切换：评审属性 / 图片信息 / 标注列表 / 标签
    ui.horizontal_wrapped(|ui| {
      for tab in RightTab::all() {
        if widgets::toggle_chip(ui, tab.label(), self.right_tab == tab, true) {
          self.right_tab = tab;
        }
      }
    });
    ui.add_space(10.0);

    match self.right_tab {
      RightTab::Review => self.tab_review(ui, dark),
      RightTab::Info => self.tab_info(ui),
      RightTab::Annotations => self.tab_annotations(ui, dark),
      RightTab::Tags => self.tab_tags(ui, dark),
    }

    ui.add_space(16.0);

    widgets::grouped_section(ui, "批量操作", |ui| {
      ui.label(
        RichText::new(format!("已选 {} 张", self.sidebar.selected_ids.len()))
          .font(theme::section_font())
          .color(theme::primary_label(dark)),
      );

      ui.horizontal(|ui| {
        widgets::section_label(ui, "目标状态");
        egui::ComboBox::from_id_salt("batch_status_target")
          .selected_text(self.batch_target_status.label())
          .show_ui(ui, |ui| {
            for s in [
              ReviewStatus::Pending,
              ReviewStatus::Approved,
              ReviewStatus::NeedsFix,
              ReviewStatus::Rejected,
            ] {
              ui.selectable_value(&mut self.batch_target_status, s, s.label());
            }
          });
      });

      ui.horizontal_wrapped(|ui| {
        if widgets::compact_secondary_button(ui, "批量更新状态", true).clicked() {
          self.dialog = Some(DialogState::ConfirmBatchOp(BatchOpKind::SetStatus(
            self.batch_target_status,
          )));
        }
        if widgets::compact_secondary_button(ui, "批量清空标注", true).clicked() {
          self.dialog = Some(DialogState::ConfirmBatchOp(BatchOpKind::ClearAnnotations));
        }
        if widgets::compact_secondary_button(ui, "复制当前标注", true).clicked() {
          self.dialog = Some(DialogState::ConfirmBatchOp(BatchOpKind::CopyCurrentAnnotations));
        }
      });

      ui.add_space(6.0);
      ui.horizontal_wrapped(|ui| {
        if widgets::toggle_chip(
          ui,
          "覆盖备注",
          self.batch_remark_mode == RemarkWriteMode::Overwrite,
          true,
        ) {
          self.batch_remark_mode = RemarkWriteMode::Overwrite;
        }
        if widgets::toggle_chip(
          ui,
          "追加备注",
          self.batch_remark_mode == RemarkWriteMode::Append,
          true,
        ) {
          self.batch_remark_mode = RemarkWriteMode::Append;
        }
      });
      if widgets::compact_secondary_button(ui, "批量写入备注", true).clicked() {
        self.dialog = Some(DialogState::ConfirmBatchOp(BatchOpKind::AddRemark));
      }

      if !self.last_batch_annotation_ids.is_empty()
        && widgets::compact_secondary_button(ui, "撤销上次批量标注", true).clicked()
      {
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
    });
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
    let result = self.service.batch_update_status(&BatchStatusRequest {
      image_ids: ids,
      target_status: target,
      confirm_irreversible: confirm,
    });
    match result {
      Ok(r) if r.applied => {
        let _ = self.reload_images();
        let _ = self.reload_batches();
        self.set_status(format!("已更新 {} 张图片状态", r.success_count));
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
        self.set_status(format!(
          "已从转换队列导入 {} 张图片到批次「{batch_name}」",
          paths.len()
        ));
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
        self
          .compare_view
          .fit_to_window(self.canvas_size_for_view(ctx));
      }
      ShortcutAction::ActualSize => {
        self
          .compare_view
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
    if let Some(item) = self.images.iter().find(|i| i.id == id) {
      self.remark_buf = item.remark.clone();
      self.properties.sync_item(item, None);
      if let Ok(meta) = self.service.refresh_metadata_cache(item.id, &item.file_path) {
        self.properties.sync_item(item, Some(meta));
      }
    }
    if let (Some(batch), Some(image)) = (self.current_batch, self.current_image) {
      let _ = self.service.save_session(batch, image);
    }
    self.load_current_annotations();
    self.reload_current_image_tags();
    if let (Some(idx), Some(batch_id)) = (self.current_index(), self.current_batch) {
      let paths: Vec<_> = self.images.iter().map(|i| i.file_path.clone()).collect();
      let thumbs: Vec<_> = self.images.iter().map(|i| i.thumbnail_path.clone()).collect();
      self.compare_view.prefetch_neighbors(
        &paths,
        idx,
        self.config.prefetch_neighbors,
        &thumbs,
      );
    }
  }

  fn select_relative(&mut self, delta: isize) {
    let Some(current) = self.current_image else { return };
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
    match self.service.set_status(id, status) {
      Ok(()) => {
        let _ = self.reload_images();
        let _ = self.reload_batches();
        self.set_status(format!("已设为「{}」", status.label()));
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
    match self.service.load_annotations(id) {
      Ok(anns) => self.current_annotations = anns,
      Err(e) => self.error = Some(e.to_string()),
    }
  }

  fn handle_canvas_events(&mut self, events: Vec<AnnotationCanvasEvent>, image_item_id: i64) {
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
          if let Err(e) = self.service.add_annotation(&ann).map(|_| ()) {
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
    }
  }

  fn reload_batches(&mut self) -> ReviewResult<()> {
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
    let Some(batch_id) = self.current_batch else {
      self.images.clear();
      return Ok(());
    };
    if self.sidebar.show_recycle {
      self.images = self.service.list_deleted_images(batch_id)?;
    } else {
      self.images = self
        .service
        .list_images(batch_id, &self.sidebar.filter)?;
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
        self.set_status(format!("已创建批次：{name}"));
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
        Ok(paths) => {
          self.set_status(format!("已导出 {} 个 JSON 到 {}", paths.len(), dir.display()))
        }
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

fn viewport_size(ctx: &egui::Context) -> egui::Vec2 {
  ctx.input(|i| {
    i.viewport()
      .inner_rect
      .map(|r| r.size())
      .unwrap_or_else(|| ctx.screen_rect().size())
  })
}

impl ReviewPanel {
  /// 按列表顺序返回已勾选图片（path, thumb, 显示名）。
  fn selected_compare_sources(&self) -> Vec<(PathBuf, Option<PathBuf>, String)> {
    self
      .images
      .iter()
      .filter(|item| self.sidebar.selected_ids.contains(&item.id))
      .map(|item| {
        let label = item
          .file_path
          .file_name()
          .map(|s| s.to_string_lossy().to_string())
          .unwrap_or_else(|| item.file_path.display().to_string());
        (
          item.file_path.clone(),
          crate::review::service::ThumbnailService::valid_cache_path(&item.file_path),
          label,
        )
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
      self.error = Some(format!("最多同时并排对比 {MAX_MULTI_COMPARE_PANES} 张，请减少选择"));
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

fn batch_op_description(op: BatchOpKind) -> &'static str {
  match op {
    BatchOpKind::SetStatus(_) => "将对所选图片批量更新评审状态，是否继续？",
    BatchOpKind::ClearAnnotations => "将清空所选图片的全部标注，是否继续？",
    BatchOpKind::AddRemark => "将对所选图片批量写入备注，是否继续？",
    BatchOpKind::CopyCurrentAnnotations => "将把当前图片的首条标注复制到所选图片，是否继续？",
  }
}

fn annotation_kind_label(kind: crate::review::domain::AnnotationKind) -> &'static str {
  use crate::review::domain::AnnotationKind;
  match kind {
    AnnotationKind::Rectangle => "矩形",
    AnnotationKind::Arrow => "箭头",
    AnnotationKind::Text => "文字",
  }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
  let chars: Vec<char> = text.chars().collect();
  if chars.len() <= max_chars {
    text.to_string()
  } else {
    format!("{}…", chars[..max_chars].iter().collect::<String>())
  }
}
