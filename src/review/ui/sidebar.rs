//! 侧边栏：批次列表、虚拟滚动图片列表、状态筛选与排序。

use eframe::egui::{self, RichText, Ui};

use crate::gui::{theme, widgets};
use crate::review::domain::{
  AnnotationFilter, BatchStats, ImageFilter, ImageSortKey, ReviewBatch, ReviewImageItem,
  ReviewStatus, TagFilterMode,
};
use crate::review::error::ReviewResult;

const ROW_HEIGHT: f32 = 64.0;
const THUMB_SIZE: f32 = 48.0;

pub struct SidebarState {
  pub filter: ImageFilter,
  pub selected_ids: Vec<i64>,
  pub batch_name_input: String,
  pub show_recycle: bool,
  /// 可选标签（供筛选栏下拉）。
  pub available_tags: Vec<crate::review::ReviewTag>,
  /// 当前列表图片-标签映射（列表色点渲染）。
  pub image_tags: std::collections::HashMap<i64, Vec<i64>>,
  /// 筛选栏展开状态。
  pub filter_expanded: bool,
}

impl Default for SidebarState {
  fn default() -> Self {
    Self {
      filter: ImageFilter::default(),
      selected_ids: Vec::new(),
      batch_name_input: String::from("新评审批次"),
      show_recycle: false,
      available_tags: Vec::new(),
      image_tags: std::collections::HashMap::new(),
      filter_expanded: false,
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
  // 快捷键：仅显示未评审 / 重置
  ui.horizontal(|ui| {
    if widgets::compact_secondary_button(ui, "仅显示未评审", true).clicked() {
      sidebar.filter.status = Some(ReviewStatus::Pending);
      changed = true;
    }
    if widgets::compact_secondary_button(ui, "重置筛选", true).clicked() {
      sidebar.filter.reset_filters();
      sidebar.show_recycle = false;
      changed = true;
    }
    let arrow = if sidebar.filter_expanded { "收起 ▲" } else { "更多筛选 ▼" };
    if widgets::compact_secondary_button(ui, arrow, true).clicked() {
      sidebar.filter_expanded = !sidebar.filter_expanded;
    }
  });

  if sidebar.filter_expanded {
    if filter_advanced_ui(ui, sidebar) {
      changed = true;
    }
  }

  if ui.checkbox(&mut sidebar.show_recycle, "回收站").changed() {
    changed = true;
  }
  changed
}

/// 高级组合筛选：标注数量 / 分辨率 / 文件大小 / 标签。
fn filter_advanced_ui(ui: &mut Ui, sidebar: &mut SidebarState) -> bool {
  let mut changed = false;

  // 标注数量
  ui.horizontal(|ui| {
    ui.label("标注");
    egui::ComboBox::from_id_salt("anno_filter")
      .selected_text(sidebar.filter.annotation_filter.label())
      .show_ui(ui, |ui| {
        for f in [
          AnnotationFilter::Any,
          AnnotationFilter::None,
          AnnotationFilter::Has,
          AnnotationFilter::AtLeast,
        ] {
          if ui
            .selectable_label(sidebar.filter.annotation_filter == f, f.label())
            .clicked()
          {
            sidebar.filter.annotation_filter = f;
            changed = true;
          }
        }
      });
    if sidebar.filter.annotation_filter == AnnotationFilter::AtLeast {
      let mut min = sidebar.filter.min_annotations.unwrap_or(1);
      if ui
        .add(egui::DragValue::new(&mut min).range(1..=999))
        .changed()
      {
        sidebar.filter.min_annotations = Some(min);
        changed = true;
      }
    }
  });

  // 分辨率（宽度像素）
  ui.horizontal(|ui| {
    ui.label("宽度 ≥");
    let mut minw = sidebar.filter.min_width.unwrap_or(0) as i32;
    if ui
      .add(egui::DragValue::new(&mut minw).range(0..=100000).suffix("px"))
      .changed()
    {
      sidebar.filter.min_width = if minw <= 0 { None } else { Some(minw as u32) };
      changed = true;
    }
    ui.label("≤");
    let mut maxw = sidebar.filter.max_width.unwrap_or(0) as i32;
    if ui
      .add(egui::DragValue::new(&mut maxw).range(0..=100000).suffix("px"))
      .changed()
    {
      sidebar.filter.max_width = if maxw <= 0 { None } else { Some(maxw as u32) };
      changed = true;
    }
  });

  // 分辨率（高度像素）
  ui.horizontal(|ui| {
    ui.label("高度 ≥");
    let mut minh = sidebar.filter.min_height.unwrap_or(0) as i32;
    if ui
      .add(egui::DragValue::new(&mut minh).range(0..=100000).suffix("px"))
      .changed()
    {
      sidebar.filter.min_height = if minh <= 0 { None } else { Some(minh as u32) };
      changed = true;
    }
    ui.label("≤");
    let mut maxh = sidebar.filter.max_height.unwrap_or(0) as i32;
    if ui
      .add(egui::DragValue::new(&mut maxh).range(0..=100000).suffix("px"))
      .changed()
    {
      sidebar.filter.max_height = if maxh <= 0 { None } else { Some(maxh as u32) };
      changed = true;
    }
  });

  // 文件大小（MB）
  ui.horizontal(|ui| {
    ui.label("大小 ≥");
    let mut min_mb = sidebar
      .filter
      .min_file_size
      .map(|b| (b as f64 / (1024.0 * 1024.0)) as f32)
      .unwrap_or(0.0);
    if ui
      .add(egui::DragValue::new(&mut min_mb).range(0.0..=100000.0).suffix("MB").speed(0.5))
      .changed()
    {
      sidebar.filter.min_file_size = if min_mb <= 0.0 {
        None
      } else {
        Some((min_mb as f64 * 1024.0 * 1024.0) as u64)
      };
      changed = true;
    }
    ui.label("≤");
    let mut max_mb = sidebar
      .filter
      .max_file_size
      .map(|b| (b as f64 / (1024.0 * 1024.0)) as f32)
      .unwrap_or(0.0);
    if ui
      .add(egui::DragValue::new(&mut max_mb).range(0.0..=100000.0).suffix("MB").speed(0.5))
      .changed()
    {
      sidebar.filter.max_file_size = if max_mb <= 0.0 {
        None
      } else {
        Some((max_mb as f64 * 1024.0 * 1024.0) as u64)
      };
      changed = true;
    }
  });

  // 标签多选
  if !sidebar.available_tags.is_empty() {
    ui.horizontal(|ui| {
      ui.label("标签模式");
      for m in [TagFilterMode::Any, TagFilterMode::All] {
        if widgets::toggle_chip(ui, m.label(), sidebar.filter.tag_mode == m, true) {
          sidebar.filter.tag_mode = m;
          changed = true;
        }
      }
    });
    let tags = sidebar.available_tags.clone();
    ui.horizontal_wrapped(|ui| {
      for tag in &tags {
        let on = sidebar.filter.tag_ids.contains(&tag.id);
        if widgets::colored_toggle_chip(ui, &tag.name, tag.color, on, true) {
          if on {
            sidebar.filter.tag_ids.retain(|t| *t != tag.id);
          } else {
            sidebar.filter.tag_ids.push(tag.id);
          }
          changed = true;
        }
      }
    });
  }

  changed
}

pub fn image_list_ui(
  ui: &mut Ui,
  ctx: &egui::Context,
  images: &[ReviewImageItem],
  current: Option<i64>,
  sidebar: &mut SidebarState,
  thumbs: &mut crate::review::ui::ListThumbnailCache,
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
        thumbs.request(img.id, &img.file_path);

        let name = img
          .file_path
          .file_name()
          .map(|s| s.to_string_lossy().to_string())
          .unwrap_or_else(|| img.file_path.display().to_string());
        let multi = sidebar.selected_ids.contains(&img.id);
        let selected = current == Some(img.id);

        ui.horizontal(|ui| {
          let mut checked = multi;
          if ui.checkbox(&mut checked, "").changed() {
            if checked {
              sidebar.selected_ids.push(img.id);
            } else {
              sidebar.selected_ids.retain(|id| *id != img.id);
            }
          }

          let (thumb_rect, thumb_resp) =
            ui.allocate_exact_size(egui::vec2(THUMB_SIZE, THUMB_SIZE), egui::Sense::click());
          if let Some(tex) = thumbs.get(img.id) {
            ui.painter().image(
              tex.id(),
              thumb_rect,
              egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
              egui::Color32::WHITE,
            );
          } else {
            ui.painter().rect_filled(
              thumb_rect,
              4.0,
              theme::control_fill(dark),
            );
          }

          // 状态色点：缩略图右下角
          widgets::status_dot(
            ui,
            egui::pos2(thumb_rect.right() - 6.0, thumb_rect.bottom() - 6.0),
            img.status.color_rgba(),
            5.0,
          );

          // 标签色点：缩略图左上角，最多 3 个
          if let Some(tag_ids) = sidebar.image_tags.get(&img.id) {
            for (i, tid) in tag_ids.iter().take(3).enumerate() {
              if let Some(tag) = sidebar.available_tags.iter().find(|t| t.id == *tid) {
                let c = tag.color;
                ui.painter().circle_filled(
                  egui::pos2(thumb_rect.left() + 6.0 + i as f32 * 9.0, thumb_rect.top() + 6.0),
                  4.0,
                  egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                );
              }
            }
          }

          ui.vertical(|ui| {
            let name_resp = ui.selectable_label(selected, &name);
            if img.annotation_count > 0 {
              ui.label(
                RichText::new(format!("{} 条标注", img.annotation_count))
                  .size(11.0)
                  .color(theme::secondary_label(dark)),
              );
            }
            let mut hover = img.status.label().to_string();
            if let Some(tag_ids) = sidebar.image_tags.get(&img.id) {
              let names: Vec<String> = tag_ids
                .iter()
                .filter_map(|tid| {
                  sidebar
                    .available_tags
                    .iter()
                    .find(|t| t.id == *tid)
                    .map(|t| t.name.clone())
                })
                .collect();
              if !names.is_empty() {
                hover = format!("{} · {}", hover, names.join("、"));
              }
            }
            name_resp.clone().on_hover_text(hover);
            if name_resp.clicked() || thumb_resp.clicked() {
              picked = Some(img.id);
            }
          });
        });
      }

      ui.add_space((total.saturating_sub(last)) as f32 * ROW_HEIGHT);

      if thumbs.poll(ctx) {
        ctx.request_repaint();
      }
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
      if widgets::colored_toggle_chip(ui, s.label(), s.color_rgba(), current == Some(s), true) {
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
