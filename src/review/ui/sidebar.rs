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
const MAX_THUMB_REQUESTS_PER_FRAME: usize = 4;

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
    ui.set_width(ui.available_width());
    egui::ScrollArea::vertical()
        .id_salt("review_batch_list")
        .max_height(160.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
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
                let row = ui.add_sized(
                    egui::vec2(ui.available_width(), 22.0),
                    egui::SelectableLabel::new(selected, label),
                );
                if row.clicked() {
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
    ui.set_max_width(ui.available_width());
    ui.set_width(ui.available_width());

    const LABEL_W: f32 = 36.0;
    let gap = 6.0;
    let row_h = widgets::TOOLBAR_ROW_HEIGHT;

    // 筛选：固定标签列 + 拉满剩余宽度的下拉
    widgets::equal_height_row(ui, gap, |ui| {
        ui.add_sized(
            egui::vec2(LABEL_W, row_h),
            egui::Label::new(
                RichText::new("筛选")
                    .size(13.0)
                    .color(theme::primary_label(ui.style().visuals.dark_mode)),
            ),
        );
        let selected = sidebar.filter.status.map(|s| s.label()).unwrap_or("全部");
        let combo_w = ui.available_width().max(80.0);
        widgets::toolbar_combo_box(ui, "status_filter", selected, combo_w, |ui| {
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

    ui.add_space(gap);

    // 排序：下拉 + 升序芯片对齐同一行高
    widgets::equal_height_row(ui, gap, |ui| {
        ui.add_sized(
            egui::vec2(LABEL_W, row_h),
            egui::Label::new(
                RichText::new("排序")
                    .size(13.0)
                    .color(theme::primary_label(ui.style().visuals.dark_mode)),
            ),
        );
        let asc_w = 56.0;
        let combo_w = (ui.available_width() - asc_w - gap).max(72.0);
        widgets::toolbar_combo_box(
            ui,
            "sort_key",
            sidebar.filter.sort_by.label(),
            combo_w,
            |ui| {
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
            },
        );
        if widgets::tab_chip_sized(ui, "升序", asc_w, sidebar.filter.sort_asc, true) {
            sidebar.filter.sort_asc = !sidebar.filter.sort_asc;
            changed = true;
        }
    });

    ui.add_space(gap);

    // 搜索 / 备注：同宽同高输入框
    if widgets::toolbar_search_edit(
        ui,
        &mut sidebar.filter.search,
        "搜索文件名…",
        ui.available_width(),
    )
    .changed()
    {
        changed = true;
    }
    ui.add_space(gap);
    if widgets::toolbar_search_edit(
        ui,
        &mut sidebar.filter.remark_contains,
        "备注包含…",
        ui.available_width(),
    )
    .changed()
    {
        changed = true;
    }

    ui.add_space(8.0);

    // 操作区：2×2 等分网格（回收站并入按钮区）
    let cell = ((ui.available_width() - gap) * 0.5).max(64.0);
    widgets::equal_height_row(ui, gap, |ui| {
        if widgets::full_width_secondary_button_in(ui, "未评审", true, cell).clicked() {
            sidebar.filter.status = Some(ReviewStatus::Pending);
            changed = true;
        }
        if widgets::full_width_secondary_button_in(ui, "重置", true, cell).clicked() {
            sidebar.filter.reset_filters();
            sidebar.show_recycle = false;
            changed = true;
        }
    });
    ui.add_space(gap);
    widgets::equal_height_row(ui, gap, |ui| {
        let more = if sidebar.filter_expanded {
            "收起"
        } else {
            "更多"
        };
        if widgets::full_width_secondary_button_in(ui, more, true, cell).clicked() {
            sidebar.filter_expanded = !sidebar.filter_expanded;
        }
        if widgets::tab_chip_sized(ui, "回收站", cell, sidebar.show_recycle, true) {
            sidebar.show_recycle = !sidebar.show_recycle;
            changed = true;
        }
    });

    if sidebar.filter_expanded {
        ui.add_space(8.0);
        widgets::inset_separator(ui);
        if filter_advanced_ui(ui, sidebar) {
            changed = true;
        }
    }

    changed
}

/// 高级组合筛选：标注数量 / 分辨率 / 文件大小 / 标签。
fn filter_advanced_ui(ui: &mut Ui, sidebar: &mut SidebarState) -> bool {
    let mut changed = false;
    const LABEL_W: f32 = 36.0;
    let gap = 6.0;
    let row_h = widgets::TOOLBAR_ROW_HEIGHT;
    let dark = ui.style().visuals.dark_mode;

    // 标注：与上方「筛选」同款标签列 + 全宽下拉
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        filter_side_label(ui, "标注", LABEL_W, row_h, dark);

        let show_min = sidebar.filter.annotation_filter == AnnotationFilter::AtLeast;
        let count_w = if show_min { 64.0 } else { 0.0 };
        let combo_w = (ui.available_width() - count_w - if show_min { gap } else { 0.0 }).max(72.0);
        widgets::toolbar_combo_box(
            ui,
            "anno_filter",
            sidebar.filter.annotation_filter.label(),
            combo_w,
            |ui| {
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
            },
        );
        if show_min {
            let mut min = sidebar.filter.min_annotations.unwrap_or(1);
            if ui
                .add_sized(
                    egui::vec2(count_w, row_h),
                    egui::DragValue::new(&mut min).range(1..=999).suffix("条"),
                )
                .changed()
            {
                sidebar.filter.min_annotations = Some(min);
                changed = true;
            }
        }
    });

    ui.add_space(gap);

    // 宽度 / 高度 / 大小：标签 + 等宽 min–max，去掉重复 ≥≤ 符号
    if filter_range_i32(
        ui,
        "宽度",
        LABEL_W,
        gap,
        row_h,
        dark,
        sidebar.filter.min_width.unwrap_or(0) as i32,
        sidebar.filter.max_width.unwrap_or(0) as i32,
        "px",
        |min, max| {
            sidebar.filter.min_width = if min <= 0 { None } else { Some(min as u32) };
            sidebar.filter.max_width = if max <= 0 { None } else { Some(max as u32) };
        },
    ) {
        changed = true;
    }
    ui.add_space(gap);
    if filter_range_i32(
        ui,
        "高度",
        LABEL_W,
        gap,
        row_h,
        dark,
        sidebar.filter.min_height.unwrap_or(0) as i32,
        sidebar.filter.max_height.unwrap_or(0) as i32,
        "px",
        |min, max| {
            sidebar.filter.min_height = if min <= 0 { None } else { Some(min as u32) };
            sidebar.filter.max_height = if max <= 0 { None } else { Some(max as u32) };
        },
    ) {
        changed = true;
    }
    ui.add_space(gap);

    let min_mb = sidebar
        .filter
        .min_file_size
        .map(|b| (b as f64 / (1024.0 * 1024.0)) as f32)
        .unwrap_or(0.0);
    let max_mb = sidebar
        .filter
        .max_file_size
        .map(|b| (b as f64 / (1024.0 * 1024.0)) as f32)
        .unwrap_or(0.0);
    if filter_range_f32(
        ui,
        "大小",
        LABEL_W,
        gap,
        row_h,
        dark,
        min_mb,
        max_mb,
        "MB",
        0.5,
        |min, max| {
            sidebar.filter.min_file_size = if min <= 0.0 {
                None
            } else {
                Some((min as f64 * 1024.0 * 1024.0) as u64)
            };
            sidebar.filter.max_file_size = if max <= 0.0 {
                None
            } else {
                Some((max as f64 * 1024.0 * 1024.0) as u64)
            };
        },
    ) {
        changed = true;
    }

    // 标签：模式等分 + 色点芯片换行
    if !sidebar.available_tags.is_empty() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            filter_side_label(ui, "标签", LABEL_W, row_h, dark);
            let cell = ((ui.available_width() - gap) * 0.5).max(48.0);
            for m in [TagFilterMode::Any, TagFilterMode::All] {
                if widgets::tab_chip_sized(ui, m.label(), cell, sidebar.filter.tag_mode == m, true)
                {
                    sidebar.filter.tag_mode = m;
                    changed = true;
                }
            }
        });
        ui.add_space(gap);
        let tags = sidebar.available_tags.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.spacing_mut().item_spacing.y = gap;
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

fn filter_side_label(ui: &mut Ui, text: &str, width: f32, height: f32, dark: bool) {
    ui.add_sized(
        egui::vec2(width, height),
        egui::Label::new(
            RichText::new(text)
                .size(13.0)
                .color(theme::primary_label(dark)),
        ),
    );
}

fn filter_range_i32(
    ui: &mut Ui,
    label: &str,
    label_w: f32,
    gap: f32,
    row_h: f32,
    dark: bool,
    mut min: i32,
    mut max: i32,
    suffix: &str,
    mut commit: impl FnMut(i32, i32),
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        filter_side_label(ui, label, label_w, row_h, dark);
        let dash_w = 12.0;
        let field = ((ui.available_width() - dash_w - gap * 2.0) * 0.5).max(48.0);
        if ui
            .add_sized(
                egui::vec2(field, row_h),
                egui::DragValue::new(&mut min)
                    .range(0..=100_000)
                    .suffix(suffix),
            )
            .changed()
        {
            changed = true;
        }
        ui.add_sized(
            egui::vec2(dash_w, row_h),
            egui::Label::new(
                RichText::new("–")
                    .size(13.0)
                    .color(theme::secondary_label(dark)),
            )
            .halign(egui::Align::Center),
        );
        if ui
            .add_sized(
                egui::vec2(field, row_h),
                egui::DragValue::new(&mut max)
                    .range(0..=100_000)
                    .suffix(suffix),
            )
            .changed()
        {
            changed = true;
        }
        if changed {
            commit(min, max);
        }
    });
    changed
}

fn filter_range_f32(
    ui: &mut Ui,
    label: &str,
    label_w: f32,
    gap: f32,
    row_h: f32,
    dark: bool,
    mut min: f32,
    mut max: f32,
    suffix: &str,
    speed: f32,
    mut commit: impl FnMut(f32, f32),
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        filter_side_label(ui, label, label_w, row_h, dark);
        let dash_w = 12.0;
        let field = ((ui.available_width() - dash_w - gap * 2.0) * 0.5).max(48.0);
        if ui
            .add_sized(
                egui::vec2(field, row_h),
                egui::DragValue::new(&mut min)
                    .range(0.0..=100_000.0)
                    .suffix(suffix)
                    .speed(speed),
            )
            .changed()
        {
            changed = true;
        }
        ui.add_sized(
            egui::vec2(dash_w, row_h),
            egui::Label::new(
                RichText::new("–")
                    .size(13.0)
                    .color(theme::secondary_label(dark)),
            )
            .halign(egui::Align::Center),
        );
        if ui
            .add_sized(
                egui::vec2(field, row_h),
                egui::DragValue::new(&mut max)
                    .range(0.0..=100_000.0)
                    .suffix(suffix)
                    .speed(speed),
            )
            .changed()
        {
            changed = true;
        }
        if changed {
            commit(min, max);
        }
    });
    changed
}

fn render_image_list_row(
    ui: &mut Ui,
    dark: bool,
    img: &ReviewImageItem,
    current: Option<i64>,
    sidebar: &mut SidebarState,
    thumbs: &mut crate::review::ui::ListThumbnailCache,
    thumb_requests: &mut usize,
) -> Option<i64> {
    if thumbs.get(img.id).is_none() && *thumb_requests < MAX_THUMB_REQUESTS_PER_FRAME {
        thumbs.request(img.id, &img.file_path);
        *thumb_requests += 1;
    }

    let name = img
        .file_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| img.file_path.display().to_string());
    let multi = sidebar.selected_ids.contains(&img.id);
    let selected = current == Some(img.id);
    let mut picked = None;

    ui.set_min_height(ROW_HEIGHT);
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
            ui.painter()
                .rect_filled(thumb_rect, 4.0, theme::control_fill(dark));
        }

        widgets::status_dot(
            ui,
            egui::pos2(thumb_rect.right() - 6.0, thumb_rect.bottom() - 6.0),
            img.status.color_rgba(),
            5.0,
        );

        if let Some(tag_ids) = sidebar.image_tags.get(&img.id) {
            for (i, tid) in tag_ids.iter().take(3).enumerate() {
                if let Some(tag) = sidebar.available_tags.iter().find(|t| t.id == *tid) {
                    let c = tag.color;
                    ui.painter().circle_filled(
                        egui::pos2(
                            thumb_rect.left() + 6.0 + i as f32 * 9.0,
                            thumb_rect.top() + 6.0,
                        ),
                        4.0,
                        egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                    );
                }
            }
        }

        ui.vertical(|ui| {
            let name_resp = ui.selectable_label(selected, &name);
            ui.horizontal(|ui| {
                if img.annotation_count > 0 {
                    ui.label(
                        RichText::new(format!("{} 条标注", img.annotation_count))
                            .size(11.0)
                            .color(theme::secondary_label(dark)),
                    );
                }
                if let Some(key) = img.jira_issue_key.as_ref().filter(|k| !k.is_empty()) {
                    let url = img
                        .jira_url
                        .clone()
                        .unwrap_or_else(|| format!("jira://{key}"));
                    if ui
                        .link(RichText::new(key).size(11.0))
                        .on_hover_text("打开 JIRA")
                        .clicked()
                    {
                        if let Some(browse) = &img.jira_url {
                            let _ = open::that(browse);
                        } else {
                            let _ = open::that(&url);
                        }
                    }
                }
            });
            let mut hover = img.status.label().to_string();
            if let Some(key) = &img.jira_issue_key {
                hover = format!("{hover} · JIRA {key}");
            }
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

    picked
}

pub fn image_list_ui(
    ui: &mut Ui,
    ctx: &egui::Context,
    images: &[ReviewImageItem],
    current: Option<i64>,
    sidebar: &mut SidebarState,
    thumbs: &mut crate::review::ui::ListThumbnailCache,
) -> ImageListAction {
    let mut filter_changed = filter_sort_ui(ui, sidebar);

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    let mut body = image_list_body_ui(ui, ctx, images, current, sidebar, thumbs, None);
    if sidebar.filter.search.len() == 1 {
        filter_changed = true;
    }
    body.reload = body.reload || filter_changed;
    body
}

/// 仅图片列表（不含筛选栏）。`list_max_h` 限制虚拟列表高度，供定高侧栏使用。
pub fn image_list_body_ui(
    ui: &mut Ui,
    ctx: &egui::Context,
    images: &[ReviewImageItem],
    current: Option<i64>,
    sidebar: &mut SidebarState,
    thumbs: &mut crate::review::ui::ListThumbnailCache,
    list_max_h: Option<f32>,
) -> ImageListAction {
    let dark = ui.style().visuals.dark_mode;
    let mut picked = None;
    let total = images.len();
    let scroll_id = ui.id().with("review_image_list");

    if images.is_empty() {
        let list_h = match list_max_h {
            Some(h) => h.max(48.0),
            // 非定高（整页滚动）时只留一小段沉底，避免把页面撑得过高
            None => ui.available_height().clamp(56.0, 120.0),
        };
        let width = ui.available_width().min(ui.max_rect().width()).max(80.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width, list_h),
            egui::Layout::bottom_up(egui::Align::Center),
            |ui| {
                ui.set_width(width);
                ui.add_space(8.0);
                ui.label(
                    RichText::new("暂无图片")
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                );
            },
        );
    } else {
        let mut thumb_requests = 0usize;
        let list_h = list_max_h
            .unwrap_or_else(|| ui.available_height())
            .max(96.0);
        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .max_height(list_h)
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, total, |ui, row_range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in row_range {
                    if let Some(id) = render_image_list_row(
                        ui,
                        dark,
                        &images[row],
                        current,
                        sidebar,
                        thumbs,
                        &mut thumb_requests,
                    ) {
                        picked = Some(id);
                    }
                }
            });

        if thumbs.poll(ctx) {
            ctx.request_repaint_after(std::time::Duration::from_millis(48));
        }
    }

    ImageListAction {
        reload: false,
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
    for s in ReviewStatus::all() {
        if widgets::colored_toggle_chip(ui, s.label(), s.color_rgba(), current == Some(s), true) {
            picked = Some(s);
        }
    }
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
