//! 侧边栏：批次列表、图片列表、状态筛选。

use eframe::egui::{self, RichText, Ui};

use crate::gui::{theme, widgets};
use crate::review::domain::{BatchStats, ImageFilter, ReviewBatch, ReviewImageItem, ReviewStatus};
use crate::review::error::ReviewResult;

pub struct SidebarState {
  pub filter: ImageFilter,
  pub selected_ids: Vec<i64>,
  pub batch_name_input: String,
}

impl Default for SidebarState {
  fn default() -> Self {
    Self {
      filter: ImageFilter::default(),
      selected_ids: Vec::new(),
      batch_name_input: String::from("新评审批次"),
    }
  }
}

pub fn batch_list_ui(
  ui: &mut Ui,
  batches: &[ReviewBatch],
  stats_cache: &[(i64, BatchStats)],
  current: Option<i64>,
) -> Option<i64> {
  let dark = ui.style().visuals.dark_mode;
  let mut picked = None;
  egui::ScrollArea::vertical()
    .id_salt("review_batch_list")
    .max_height(160.0)
    .show(ui, |ui| {
      for batch in batches {
        let stats = stats_cache
          .iter()
          .find(|(id, _)| *id == batch.id)
          .map(|(_, s)| s);
        let label = if let Some(s) = stats {
          format!(
            "{} · {} 张 · 通过 {}",
            batch.name, batch.total_count, s.approved
          )
        } else {
          format!("{} · {} 张", batch.name, batch.total_count)
        };
        let selected = current == Some(batch.id);
        if ui.selectable_label(selected, label).clicked() {
          picked = Some(batch.id);
        }
      }
      if batches.is_empty() {
        ui.label(
          RichText::new("暂无批次")
            .size(12.0)
            .color(theme::secondary_label(dark)),
        );
      }
    });
  picked
}

pub fn image_list_ui(
  ui: &mut Ui,
  images: &[ReviewImageItem],
  current: Option<i64>,
  sidebar: &mut SidebarState,
) -> ImageListAction {
  let dark = ui.style().visuals.dark_mode;

  ui.horizontal(|ui| {
    widgets::section_label(ui, "筛选");
    egui::ComboBox::from_id_salt("status_filter")
      .selected_text(
        sidebar
          .filter
          .status
          .map(|s| s.label())
          .unwrap_or("全部"),
      )
      .show_ui(ui, |ui| {
        if ui
          .selectable_label(sidebar.filter.status.is_none(), "全部")
          .clicked()
        {
          sidebar.filter.status = None;
        }
        for s in [
          ReviewStatus::Pending,
          ReviewStatus::Approved,
          ReviewStatus::NeedsFix,
          ReviewStatus::Rejected,
        ] {
          if ui
            .selectable_label(sidebar.filter.status == Some(s), s.label())
            .clicked()
          {
            sidebar.filter.status = Some(s);
          }
        }
      });
  });

  ui.add(
    egui::TextEdit::singleline(&mut sidebar.filter.search)
      .hint_text("搜索文件名…")
      .margin(egui::vec2(12.0, 10.0)),
  );

  let mut filter_changed = false;
  if sidebar.filter.search.len() == 1 {
    filter_changed = true;
  }

  ui.add_space(4.0);
  ui.separator();
  ui.add_space(4.0);

  let mut picked = None;
  egui::ScrollArea::vertical()
    .id_salt("review_image_list")
    .show(ui, |ui| {
      for img in images {
        let name = img
          .file_path
          .file_name()
          .map(|s| s.to_string_lossy().to_string())
          .unwrap_or_else(|| img.file_path.display().to_string());
        let row = format!("[{}] {}", img.status.label(), name);
        let multi = sidebar.selected_ids.contains(&img.id);
        ui.horizontal(|ui| {
          let mut checked = multi;
          if ui.checkbox(&mut checked, "").changed() {
            if checked {
              sidebar.selected_ids.push(img.id);
            } else {
              sidebar.selected_ids.retain(|id| *id != img.id);
            }
          }
          let selected = current == Some(img.id);
          if ui.selectable_label(selected, row).clicked() {
            picked = Some(img.id);
          }
        });
      }
      if images.is_empty() {
        ui.label(
          RichText::new("暂无图片")
            .size(12.0)
            .color(theme::secondary_label(dark)),
        );
      }
    });
  ImageListAction {
    reload: filter_changed,
    selected: picked,
  }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ImageListAction {
  pub reload: bool,
  pub selected: Option<i64>,
}

pub fn status_buttons(ui: &mut Ui, current: Option<ReviewStatus>) -> Option<ReviewStatus> {
  let mut picked = None;
  ui.horizontal_wrapped(|ui| {
    for s in [
      ReviewStatus::Pending,
      ReviewStatus::Approved,
      ReviewStatus::NeedsFix,
      ReviewStatus::Rejected,
    ] {
      if widgets::toggle_chip(ui, s.label(), current == Some(s), true) {
        picked = Some(s);
      }
    }
  });
  picked
}

pub fn format_stats(stats: &BatchStats) -> String {
  format!(
    "未评审 {} · 通过 {} · 待修正 {} · 驳回 {}",
    stats.pending, stats.approved, stats.needs_fix, stats.rejected
  )
}

#[allow(dead_code)]
pub fn reload_hint(result: &ReviewResult<()>) {
  if let Err(e) = result {
    tracing::warn!("sidebar action failed: {e}");
  }
}
