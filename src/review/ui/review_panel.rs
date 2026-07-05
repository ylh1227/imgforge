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
use crate::review::ui::compare_view::CompareView;
use crate::review::ui::shortcuts::handle_shortcuts;
use crate::review::domain::image_item::next_image_id;
use crate::review::service::ReviewModuleConfig;
use crate::review::ui::properties_panel::{properties_panel_ui, PropertiesPanelState};
use crate::review::ui::shortcut_panel::{shortcut_panel_ui, ShortcutPanelState};
use crate::review::ui::sidebar::{
  batch_list_ui, format_stats, image_list_ui, status_buttons, SidebarState,
};
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
    };
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

      let approved = self
        .current_batch
        .map(|id| {
          self
            .batch_stats
            .iter()
            .find(|(bid, _)| *bid == id)
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
        self.left_column(ui, dark);
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
      self.left_column(ui, dark);
      ui.separator();
      self.center_column(ui, ctx, host);
      ui.separator();
      self.right_column(ui, dark);
    });
  }

  fn left_column(&mut self, ui: &mut egui::Ui, dark: bool) {
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
        let _ = self.reload_images();
      }
    });

    ui.add_space(16.0);

    widgets::grouped_section(ui, "图片", |ui| {
      let list_action =
        image_list_ui(ui, &self.images, self.current_image, &mut self.sidebar);
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
    widgets::grouped_section(ui, "画布", |ui| {
      if let Some(item) = self.current_item().cloned() {
        let thumb = self
          .service
          .ensure_thumbnail(item.id, &item.file_path)
          .ok();
        let thumb_ref = thumb.as_deref();
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

  fn right_column(&mut self, ui: &mut egui::Ui, dark: bool) {
    widgets::grouped_section(ui, "属性", |ui| {
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
    });

    ui.add_space(16.0);

    widgets::grouped_section(ui, "当前图片", |ui| {
      let current_status = self.current_item().map(|item| item.status);
      if let Some(status) = status_buttons(ui, current_status) {
        if let Some(id) = self.current_image {
          self.set_image_status(id, status);
        }
      }

      ui.add_space(8.0);
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
      }

      ui.add_space(8.0);
      if ui
        .checkbox(
          &mut self.config.auto_advance_on_status,
          "切换状态后跳下一张未评审",
        )
        .changed()
      {
        let _ = self.config.save();
      }
    });

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
    match self.service.approved_paths(batch_id) {
      Ok(paths) => {
        let n = paths.len();
        self.output.enqueue_approved = paths;
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
        self.compare_view.fit_to_window(viewport_size(ctx));
      }
      ShortcutAction::ActualSize => {
        self.compare_view.set_zoom_100(viewport_size(ctx));
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
          if let Some(next) = next_image_id(
            &self.images,
            id,
            self.config.auto_advance_target,
          ) {
            self.select_image(next);
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
    }
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

fn batch_op_description(op: BatchOpKind) -> &'static str {
  match op {
    BatchOpKind::SetStatus(_) => "将对所选图片批量更新评审状态，是否继续？",
    BatchOpKind::ClearAnnotations => "将清空所选图片的全部标注，是否继续？",
    BatchOpKind::AddRemark => "将对所选图片批量写入备注，是否继续？",
    BatchOpKind::CopyCurrentAnnotations => "将把当前图片的首条标注复制到所选图片，是否继续？",
  }
}
