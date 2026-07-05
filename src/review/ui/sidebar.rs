//! 侧边栏：批次列表、虚拟滚动图片列表、状态筛选与排序。

use eframe::egui::{self, RichText, Ui};

use crate::gui::{theme, widgets};
use crate::review::domain::{BatchStats, ImageFilter, ImageSortKey, ReviewBatch, ReviewImageItem, ReviewStatus};
use crate::review::error::ReviewResult;

const ROW_HEIGHT: f32 = 28.0;

pub struct SidebarState {
  pub filter: ImageFilter,
  pub selected_ids: Vec<i64>,
  pub batch_name_input: String,
  pub show_recycle: bool,
}

impl Default for SidebarState {
  fn default() -> Self {
    Self {
      filter: ImageFilter::default(),
      selected_ids: Vec::new(),
      batch_name_input: String::from("新评审批次"),
      show_recycle: false,
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

pub fn filter_sort_ui(ui: &mut Ui, sidebar: &mut SidebarState) -> bool {
  let mut changed = false;
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
          changed = true;
        }
        for s in ReviewStatus::all() {
          if ui
            .selectable_label(sidebar.filter.status == Some(s), s.label())
            .clicked()
          {
            sidebar.filter.status = Some(s);
            changed = true;
          }
        }
      });
  });

  ui.horizontal(|ui| {
    widgets::section_label(ui, "排序");
    egui::ComboBox::from_id_salt("sort_key")
      .selected_text(sidebar.filter.sort_by.label())
      .show_ui(ui, |ui| {
        for key in [
          ImageSortKey::FilePath,
          ImageSortKey::Status,
          ImageSortKey::UpdatedAt,
          ImageSortKey::FileSize,
          ImageSortKey::Resolution,
          ImageSortKey::AnnotationCount,
        ] {
          if ui
            .selectable_label(sidebar.filter.sort_by == key, key.label())
            .clicked()
          {
            sidebar.filter.sort_by = key;
            changed = true;
          }
        }
      });
    if ui.checkbox(&mut sidebar.filter.sort_asc, "升序").changed() {
      changed = true;
    }
  });

  ui.add(
    egui::TextEdit::singleline(&mut sidebar.filter.search)
      .hint_text("搜索文件名…")
      .margin(egui::vec2(12.0, 10.0)),
  );
  ui.horizontal(|ui| {
    ui.label("备注包含");
    if ui
      .text_edit_singleline(&mut sidebar.filter.remark_contains)
      .changed()
    {
      changed = true;
    }
  });
  ui.horizontal(|ui| {
    ui.label("最少标注");
    let mut min = sidebar.filter.min_annotations.unwrap_or(0);
    if ui
      .add(egui::DragValue::new(&mut min).range(0..=999))
      .changed()
    {
      sidebar.filter.min_annotations = if min == 0 { None } else { Some(min) };
      changed = true;
    }
  });
  if ui.checkbox(&mut sidebar.show_recycle, "回收站").changed() {
    changed = true;
  }
  changed
}

pub fn image_list_ui(
  ui: &mut Ui,
  images: &[ReviewImageItem],
  current: Option<i64>,
  sidebar: &mut SidebarState,
) -> ImageListAction {
  let dark = ui.style().visuals.dark_mode;
  let mut filter_changed = filter_sort_ui(ui, sidebar);

  ui.add_space(4.0);
  ui.separator();
  ui.add_space(4.0);

  let mut picked = None;
  let total = images.len();
  let scroll_id = ui.id().with("review_image_list");

  egui::ScrollArea::vertical()
    .id_salt(scroll_id)
    .show(ui, |ui| {
      if images.is_empty() {
        ui.label(
          RichText::new("暂无图片")
            .size(12.0)
            .color(theme::secondary_label(dark)),
        );
        return;
      }

      let viewport = ui.clip_rect();
      let scroll_off = ui.min_rect().min.y - viewport.min.y;
      let first = ((scroll_off / ROW_HEIGHT).floor() as isize).max(0) as usize;
      let visible = ((viewport.height() / ROW_HEIGHT).ceil() as usize).saturating_add(2);
      let last = (first + visible).min(total);

      ui.set_min_height(total as f32 * ROW_HEIGHT);
      ui.add_space(first as f32 * ROW_HEIGHT);

      for img in &images[first..last] {
        let name = img
          .file_path
          .file_name()
          .map(|s| s.to_string_lossy().to_string())
          .unwrap_or_else(|| img.file_path.display().to_string());
        let row = format!(
          "[{}] {} ({})",
          img.status.label(),
          name,
          img.annotation_count
        );
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

      ui.add_space((total.saturating_sub(last)) as f32 * ROW_HEIGHT);
    });

  if sidebar.filter.search.len() == 1 {
    filter_changed = true;
  }

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
    for s in ReviewStatus::all() {
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
