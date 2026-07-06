//! 可复用 UI 组件（macOS 分组列表布局，随窗口自适应）。

use eframe::egui::{self, Button, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke, TextEdit, Ui};

use crate::gui::theme;

/// 工具栏统一行高（与 compact 按钮、状态芯片一致）。
pub const TOOLBAR_ROW_HEIGHT: f32 = 32.0;

/// 工具栏内按钮内边距（小于全局 `button_padding`，以便固定行高内垂直居中）。
const TOOLBAR_BUTTON_PADDING: egui::Vec2 = egui::vec2(10.0, 4.0);
/// 与 compact 按钮描边对齐，纯文本标签需补一点左距。
const TOOLBAR_STROKE_INSET: f32 = 2.0;

fn add_toolbar_sized_button(ui: &mut Ui, size: egui::Vec2, enabled: bool, btn: Button) -> egui::Response {
  ui.add_enabled_ui(enabled, |ui| ui.add_sized(size, btn)).inner
}

fn toolbar_text_width(ui: &Ui, label: &str) -> f32 {
  ui.fonts(|fonts| {
    fonts
      .layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(13.0),
        Color32::PLACEHOLDER,
      )
      .size()
      .x
  })
}

fn toolbar_button_width(ui: &Ui, label: &str) -> f32 {
  (toolbar_text_width(ui, label) + TOOLBAR_BUTTON_PADDING.x * 2.0).max(56.0)
}

/// 工具栏按钮预估宽度（用于行内剩余空间计算）。
pub fn toolbar_control_width(ui: &Ui, label: &str) -> f32 {
  toolbar_button_width(ui, label)
}

/// 工具栏单行搜索框（与 compact 按钮同高、垂直居中）。
pub fn toolbar_search_edit(
  ui: &mut Ui,
  text: &mut String,
  hint: &str,
  width: f32,
) -> egui::Response {
  ui.add_sized(
    egui::vec2(width, TOOLBAR_ROW_HEIGHT),
    TextEdit::singleline(text)
      .hint_text(hint)
      .margin(egui::vec2(8.0, 7.0)),
  )
}

/// 常用栏左区宽度：三行左组对齐（导航 / 对比模式 / 视图）。
pub fn workflow_left_zone_width(ui: &Ui, page_label: Option<&str>) -> f32 {
  let spacing = 6.0;
  let mut row1 =
    toolbar_button_width(ui, "◀ 上一张") + spacing + toolbar_button_width(ui, "下一张 ▶");
  if let Some(label) = page_label {
    row1 += spacing + toolbar_text_width(ui, label);
  }
  let row2 = TOOLBAR_STROKE_INSET + toolbar_text_width(ui, "对比模式") + spacing + 120.0;
  let row3 = toolbar_button_width(ui, "适应窗口")
    + spacing
    + toolbar_button_width(ui, "100%")
    + spacing
    + toolbar_button_width(ui, "撤销标注");
  row1.max(row2).max(row3)
}

/// 常用栏左区容器（固定宽，内容自左排列）。
pub fn toolbar_left_zone<R>(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
  ui.allocate_ui_with_layout(
    egui::vec2(width, TOOLBAR_ROW_HEIGHT),
    Layout::left_to_right(egui::Align::Center),
    |ui| {
      ui.spacing_mut().item_spacing.x = 6.0;
      add_contents(ui)
    },
  )
  .inner
}

/// 工具栏字段标签（与 compact 按钮左缘对齐）。
pub fn toolbar_field_label(ui: &mut Ui, text: &str, dark: bool) {
  ui.add_space(TOOLBAR_STROKE_INSET);
  ui.label(
    RichText::new(text)
      .size(13.0)
      .strong()
      .color(theme::primary_label(dark)),
  );
}

/// 工具栏单行：垂直居中对齐，避免 `horizontal_wrapped` 顶对齐导致错位。
pub fn toolbar_row<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
  ui.horizontal(|ui| {
    ui.spacing_mut().button_padding = TOOLBAR_BUTTON_PADDING;
    ui.set_min_height(TOOLBAR_ROW_HEIGHT);
    ui.set_width(ui.available_width());
    ui.with_layout(Layout::left_to_right(egui::Align::Center), add_contents)
      .inner
  })
  .inner
}

/// 工具栏竖向分隔线（与行高等高）。
pub fn toolbar_separator(ui: &mut Ui) {
  let dark = ui.style().visuals.dark_mode;
  ui.add_space(6.0);
  let (rect, _) = ui.allocate_exact_size(
    egui::vec2(1.0, TOOLBAR_ROW_HEIGHT),
    egui::Sense::hover(),
  );
  ui.painter()
    .vline(rect.center().x, rect.y_range(), theme::separator_stroke(dark));
  ui.add_space(6.0);
}

/// 工具栏下拉框：与 compact 按钮相同的圆角、描边与行高。
pub fn toolbar_combo_box(
  ui: &mut Ui,
  id_salt: impl std::hash::Hash,
  selected_label: &str,
  width: f32,
  add_menu: impl FnOnce(&mut Ui),
) {
  let dark = ui.style().visuals.dark_mode;
  let popup_id = ui.id().with(id_salt).with("popup");
  let is_open = ui.memory(|m| m.is_popup_open(popup_id));

  let btn = Button::new(
    RichText::new(selected_label)
      .size(13.0)
      .color(theme::primary_label(dark)),
  )
  .fill(if is_open {
    theme::accent(dark).linear_multiply(0.15)
  } else {
    theme::control_fill(dark)
  })
  .stroke(theme::control_stroke(dark))
  .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));

  let button_response =
    add_toolbar_sized_button(ui, egui::vec2(width, TOOLBAR_ROW_HEIGHT), true, btn);

  if button_response.clicked() {
    ui.memory_mut(|m| m.toggle_popup(popup_id));
  }

  let _ = egui::popup::popup_below_widget(
    ui,
    popup_id,
    &button_response,
    egui::PopupCloseBehavior::CloseOnClickOutside,
    |ui| {
      ui.set_min_width(width);
      Frame::new()
        .fill(theme::grouped_fill(dark))
        .stroke(theme::control_stroke(dark))
        .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(4, 4))
        .show(ui, add_menu);
    },
  );
}

pub fn navigation_header(ui: &mut Ui, subtitle: &str) {
  let dark = ui.style().visuals.dark_mode;
  ui.vertical(|ui| {
    ui.label(
      RichText::new("ImgForge")
        .font(theme::title_font())
        .strong()
        .color(theme::primary_label(dark)),
    );
    ui.add_space(4.0);
    ui.label(
      RichText::new(subtitle)
        .font(theme::subtitle_font())
        .color(theme::secondary_label(dark)),
    );
  });
}

/// 分组标题（与 `grouped_section` 标题样式一致）。
pub fn section_header(ui: &mut Ui, title: &str) {
  let dark = ui.style().visuals.dark_mode;
  ui.label(
    RichText::new(title)
      .font(theme::section_header_font())
      .strong()
      .color(theme::secondary_label(dark)),
  );
}

/// 分组内容框（无标题），与 `grouped_section` 内框样式一致。
pub fn grouped_section_frame<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
  let dark = ui.style().visuals.dark_mode;
  Frame::new()
    .fill(theme::grouped_fill(dark))
    .corner_radius(CornerRadius::same(theme::GROUP_RADIUS))
    .inner_margin(Margin::symmetric(16, 14))
    .show(ui, |ui| {
      ui.set_width(ui.available_width());
      add_contents(ui)
    })
    .inner
}

/// 内容层分组（inset grouped list），宽度随父级拉伸。
pub fn grouped_section<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
  section_header(ui, title);
  ui.add_space(6.0);
  grouped_section_frame(ui, add_contents)
}

/// 底部操作工具栏帧（贴合窗口背景，仅按钮保持控件层级）。
pub fn glass_toolbar_frame(dark: bool) -> Frame {
  Frame::new()
    .fill(theme::window_fill(dark))
    .stroke(Stroke::NONE)
    .shadow(theme::toolbar_shadow(dark))
    .inner_margin(Margin::symmetric(20, 10))
    .corner_radius(CornerRadius::ZERO)
}

/// egui 回退工具栏点击结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarClick {
  Start,
  Cancel,
  OpenOutput,
}

/// egui 回退工具栏：宽屏居中，窄屏自动换行。
pub fn action_toolbar_row(ui: &mut Ui, enabled: bool, running: bool) -> Option<ToolbarClick> {
  let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
  let mut clicked = None;

  ui.with_layout(
    if narrow {
      Layout::top_down(egui::Align::Center)
    } else {
      Layout::left_to_right(egui::Align::Center)
    },
    |ui| {
      if narrow {
        if primary_button(ui, "开始转换", enabled).clicked() {
          clicked = Some(ToolbarClick::Start);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
          if secondary_button(ui, "取消", running).clicked() {
            clicked = Some(ToolbarClick::Cancel);
          }
          ui.add_space(8.0);
          if secondary_button(ui, "打开输出", true).clicked() {
            clicked = Some(ToolbarClick::OpenOutput);
          }
        });
      } else {
        ui.horizontal_centered(|ui| {
          if primary_button(ui, "开始转换", enabled).clicked() {
            clicked = Some(ToolbarClick::Start);
          }
          ui.add_space(8.0);
          if secondary_button(ui, "取消", running).clicked() {
            clicked = Some(ToolbarClick::Cancel);
          }
          ui.add_space(8.0);
          if secondary_button(ui, "打开输出", true).clicked() {
            clicked = Some(ToolbarClick::OpenOutput);
          }
        });
      }
    },
  );

  clicked
}

pub fn folder_field(ui: &mut Ui, label: &str, path: &mut String, enabled: bool) {
  let dark = ui.style().visuals.dark_mode;
  let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
  const BROWSE_WIDTH: f32 = 88.0;

  if narrow {
    ui.label(
      RichText::new(label)
        .font(theme::section_font())
        .color(theme::primary_label(dark)),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
      let edit_w = (ui.available_width() - BROWSE_WIDTH - 8.0).max(80.0);
      let edit = TextEdit::singleline(path)
        .hint_text("选择或拖入文件夹…")
        .margin(egui::vec2(12.0, 10.0))
        .desired_width(edit_w);
      ui.add_enabled(enabled, edit);
      browse_button(ui, enabled, path, dark);
    });
  } else {
    ui.horizontal(|ui| {
      ui.allocate_ui_with_layout(
        egui::vec2(52.0, ui.spacing().interact_size.y),
        Layout::left_to_right(egui::Align::Center),
        |ui| {
          ui.label(
            RichText::new(label)
              .font(theme::section_font())
              .color(theme::primary_label(dark)),
          );
        },
      );
      let edit_w = (ui.available_width() - BROWSE_WIDTH - 8.0).max(120.0);
      let edit = TextEdit::singleline(path)
        .hint_text("选择或拖入文件夹…")
        .margin(egui::vec2(12.0, 10.0))
        .desired_width(edit_w);
      ui.add_enabled(enabled, edit);
      browse_button(ui, enabled, path, dark);
    });
  }

  ui.add_space(4.0);
  ui.separator();
  ui.add_space(4.0);
}

fn browse_button(ui: &mut Ui, enabled: bool, path: &mut String, dark: bool) {
  if ui
    .add_enabled(
      enabled,
      Button::new(RichText::new("浏览…").size(13.0))
        .fill(theme::control_fill(dark))
        .stroke(theme::control_stroke(dark))
        .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS)),
    )
    .clicked()
  {
    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
      *path = folder.display().to_string();
    }
  }
}

pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
  let dark = ui.style().visuals.dark_mode;
  let accent = theme::accent(dark);
  let btn = Button::new(RichText::new(label).size(15.0).strong().color(Color32::WHITE))
    .fill(if enabled {
      accent
    } else {
      accent.linear_multiply(0.45)
    })
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(140.0, 38.0));
  ui.add_enabled(enabled, btn)
}

pub fn secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
  let dark = ui.style().visuals.dark_mode;
  let btn = Button::new(RichText::new(label).size(14.0).color(theme::primary_label(dark)))
    .fill(theme::control_fill(dark))
    .stroke(theme::control_stroke(dark))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(96.0, 38.0));
  ui.add_enabled(enabled, btn)
}

/// 工具栏用紧凑次要按钮（评审操作栏等，宽度随文案）。
pub fn compact_secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
  let dark = ui.style().visuals.dark_mode;
  let btn = Button::new(RichText::new(label).size(13.0).color(theme::primary_label(dark)))
    .fill(theme::control_fill(dark))
    .stroke(theme::control_stroke(dark))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));
  add_toolbar_sized_button(
    ui,
    egui::vec2(toolbar_button_width(ui, label), TOOLBAR_ROW_HEIGHT),
    enabled,
    btn,
  )
}

/// 工具栏用紧凑主要按钮（宽度随文案）。
pub fn compact_primary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
  let dark = ui.style().visuals.dark_mode;
  let accent = theme::accent(dark);
  let btn = Button::new(RichText::new(label).size(13.0).strong().color(Color32::WHITE))
    .fill(if enabled {
      accent
    } else {
      accent.linear_multiply(0.45)
    })
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));
  add_toolbar_sized_button(
    ui,
    egui::vec2(toolbar_button_width(ui, label), TOOLBAR_ROW_HEIGHT),
    enabled,
    btn,
  )
}

/// 可选中芯片（与质量预设样式一致）。
pub fn toggle_chip(ui: &mut Ui, label: &str, selected: bool, enabled: bool) -> bool {
  let dark = ui.style().visuals.dark_mode;
  let accent = theme::accent(dark);
  let (fill, stroke, fg) = if selected {
    (
      accent.linear_multiply(0.22),
      Stroke::new(1.5, accent),
      accent,
    )
  } else {
    (
      theme::control_fill(dark),
      theme::control_stroke(dark),
      theme::primary_label(dark),
    )
  };

  let btn = Button::new(RichText::new(label).size(13.0).color(fg))
    .fill(fill)
    .stroke(stroke)
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));

  add_toolbar_sized_button(ui, egui::vec2(56.0, TOOLBAR_ROW_HEIGHT), enabled, btn).clicked()
}

/// 侧栏 Tab 芯片（宽度随文案，避免固定 56px 挤压）。
pub fn tab_chip(ui: &mut Ui, label: &str, selected: bool, enabled: bool) -> bool {
  let dark = ui.style().visuals.dark_mode;
  let accent = theme::accent(dark);
  let (fill, stroke, fg) = if selected {
    (
      accent.linear_multiply(0.22),
      Stroke::new(1.5, accent),
      accent,
    )
  } else {
    (
      theme::control_fill(dark),
      theme::control_stroke(dark),
      theme::primary_label(dark),
    )
  };

  let w = toolbar_button_width(ui, label).clamp(40.0, 68.0);
  let btn = Button::new(RichText::new(label).size(13.0).color(fg))
    .fill(fill)
    .stroke(stroke)
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));

  add_toolbar_sized_button(ui, egui::vec2(w, TOOLBAR_ROW_HEIGHT), enabled, btn).clicked()
}

/// 侧栏 Tab 芯片（指定宽度，用于网格布局）。
pub fn tab_chip_sized(
  ui: &mut Ui,
  label: &str,
  width: f32,
  selected: bool,
  enabled: bool,
) -> bool {
  let dark = ui.style().visuals.dark_mode;
  let accent = theme::accent(dark);
  let (fill, stroke, fg) = if selected {
    (
      accent.linear_multiply(0.22),
      Stroke::new(1.5, accent),
      accent,
    )
  } else {
    (
      theme::control_fill(dark),
      theme::control_stroke(dark),
      theme::primary_label(dark),
    )
  };

  let w = width.max(40.0);
  let btn = Button::new(RichText::new(label).size(13.0).color(fg))
    .fill(fill)
    .stroke(stroke)
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));

  add_toolbar_sized_button(ui, egui::vec2(w, TOOLBAR_ROW_HEIGHT), enabled, btn).clicked()
}

/// 多 Tab 网格选择（2 列等宽，在分组框外使用，避免圆角裁切）。
pub fn tab_grid_selector<R>(
  ui: &mut Ui,
  id_salt: impl std::hash::Hash,
  tabs: &[(R, &str)],
  current: R,
  mut on_select: impl FnMut(R),
) where
  R: Copy + PartialEq,
{
  const COLS: usize = 2;
  let gap = 6.0;
  let avail = (ui.available_width() - 2.0).max(128.0);
  let cell_w = ((avail - gap * (COLS as f32 - 1.0)) / COLS as f32).max(64.0);

  for (row_idx, chunk) in tabs.chunks(COLS).enumerate() {
    ui.horizontal(|ui| {
      ui.spacing_mut().item_spacing.x = gap;
      for (tab, label) in chunk {
        if tab_chip_sized(ui, label, cell_w, current == *tab, true) {
          on_select(*tab);
        }
      }
    });
    if row_idx + 1 < tabs.len().div_ceil(COLS) {
      ui.add_space(gap);
    }
  }
  let _ = id_salt;
}

/// 多 Tab 选择行：宽度不足时自动换行，极窄时退化为下拉框。
pub fn tab_selector_row<R>(
  ui: &mut Ui,
  id_salt: impl std::hash::Hash,
  tabs: &[(R, &str)],
  current: R,
  mut on_select: impl FnMut(R),
) where
  R: Copy + PartialEq,
{
  let avail = ui.available_width();
  let gap = 4.0;
  let chips_w = tabs
    .iter()
    .map(|(_, label)| toolbar_button_width(ui, label).clamp(40.0, 68.0) + gap)
    .sum::<f32>()
    - gap;

  if avail < chips_w {
    let selected = tabs
      .iter()
      .find(|(tab, _)| *tab == current)
      .map(|(_, label)| *label)
      .unwrap_or(tabs[0].1);
    toolbar_combo_box(ui, id_salt, selected, avail, |ui| {
      for (tab, label) in tabs {
        if ui.selectable_label(current == *tab, *label).clicked() {
          on_select(*tab);
        }
      }
    });
    return;
  }

  ui.horizontal_wrapped(|ui| {
    ui.set_width(avail);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    for (tab, label) in tabs {
      if tab_chip(ui, label, current == *tab, true) {
        on_select(*tab);
      }
    }
  });
}

/// 带固定色的可选中芯片：选中时用该色填充，未选中显示描边点。
pub fn colored_toggle_chip(
  ui: &mut Ui,
  label: &str,
  rgba: [u8; 4],
  selected: bool,
  enabled: bool,
) -> bool {
  let dark = ui.style().visuals.dark_mode;
  let color = Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
  let (fill, stroke, fg) = if selected {
    (color, Stroke::new(1.5, color), Color32::WHITE)
  } else {
    (
      color.linear_multiply(0.14),
      Stroke::new(1.0, color.linear_multiply(0.6)),
      theme::primary_label(dark),
    )
  };
  let btn = Button::new(RichText::new(label).size(13.0).color(fg))
    .fill(fill)
    .stroke(stroke)
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));
  add_toolbar_sized_button(ui, egui::vec2(56.0, TOOLBAR_ROW_HEIGHT), enabled, btn).clicked()
}

/// 在指定矩形右下角绘制状态色小圆点（叠加到缩略图/行）。
pub fn status_dot(ui: &Ui, center: egui::Pos2, rgba: [u8; 4], radius: f32) {
  let color = Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
  let painter = ui.painter();
  painter.circle_filled(center, radius, color);
  painter.circle_stroke(center, radius, Stroke::new(1.0, Color32::from_white_alpha(180)));
}

/// 顶部模式切换条：紧凑横向分段，宽度随内容收缩（不撑满父级）。
pub fn mode_tab_bar<T: PartialEq + Copy>(
  ui: &mut Ui,
  value: &mut T,
  options: &[(T, &str)],
) {
  if options.len() < 2 {
    return;
  }

  let dark = ui.style().visuals.dark_mode;
  let accent = theme::accent(dark);
  let seg_w = if ui.available_width() < theme::NARROW_BREAKPOINT {
    108.0
  } else {
    120.0
  };
  let seg_h = 36.0;

  ui.horizontal(|ui| {
    Frame::new()
      .fill(theme::segment_track_fill(dark))
      .stroke(theme::separator_stroke(dark))
      .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
      .inner_margin(Margin::same(4))
      .show(ui, |ui| {
        ui.horizontal(|ui| {
          ui.spacing_mut().item_spacing.x = 4.0;
          for (option, label) in options {
            let selected = *value == *option;
            let (fill, stroke, fg) = if selected {
              (accent, Stroke::NONE, Color32::WHITE)
            } else {
              (
                Color32::TRANSPARENT,
                Stroke::NONE,
                theme::primary_label(dark),
              )
            };

            let text = RichText::new(*label).size(14.0).color(fg);
            let text = if selected { text.strong() } else { text };
            let btn = Button::new(text)
              .fill(fill)
              .stroke(stroke)
              .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS.saturating_sub(2)))
              .min_size(egui::vec2(seg_w, seg_h));

            if ui.add(btn).clicked() {
              *value = *option;
            }
          }
        });
      });
  });
}

pub fn error_banner(ui: &mut Ui, text: &str) {
  let dark = ui.style().visuals.dark_mode;
  Frame::new()
    .fill(theme::error_color(dark).linear_multiply(0.12))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .inner_margin(Margin::symmetric(14, 10))
    .stroke(Stroke::new(1.0, theme::error_color(dark).linear_multiply(0.55)))
    .show(ui, |ui| {
      ui.set_width(ui.available_width());
      ui.label(
        RichText::new(text)
          .size(13.5)
          .color(theme::error_color(dark)),
      );
    });
}

pub fn section_label(ui: &mut Ui, text: &str) {
  let dark = ui.style().visuals.dark_mode;
  ui.label(
    RichText::new(text)
      .font(theme::section_font())
      .color(theme::primary_label(dark)),
  );
}

/// 转换设置区内细分组标题（如「文件选项」）。
pub fn settings_subheading(ui: &mut Ui, text: &str) {
  let dark = ui.style().visuals.dark_mode;
  ui.label(
    RichText::new(text)
      .size(12.0)
      .color(theme::secondary_label(dark)),
  );
}

/// 分组内细分隔线。
pub fn inset_separator(ui: &mut Ui) {
  ui.add_space(4.0);
  ui.separator();
  ui.add_space(4.0);
}

fn settings_label(ui: &mut Ui, text: &str, dark: bool) {
  ui.allocate_ui_with_layout(
    egui::vec2(theme::SETTINGS_LABEL_WIDTH, ui.spacing().interact_size.y),
    Layout::left_to_right(egui::Align::Center),
    |ui| {
      ui.label(
        RichText::new(text)
          .font(theme::section_font())
          .color(theme::primary_label(dark)),
      );
    },
  );
}

/// 固定标签列 + 右侧控件行（宽屏）；窄屏改为标签在上。
pub fn settings_labeled_row<R>(
  ui: &mut Ui,
  label: &str,
  add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
  let dark = ui.style().visuals.dark_mode;
  let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;

  if narrow {
    ui.label(
      RichText::new(label)
        .font(theme::section_font())
        .color(theme::primary_label(dark)),
    );
    ui.add_space(4.0);
    add_contents(ui)
  } else {
    ui.horizontal(|ui| {
      settings_label(ui, label, dark);
      ui.add_space(8.0);
      add_contents(ui)
    })
    .inner
  }
}

/// 与 [`settings_labeled_row`] 标签列对齐的缩进区域（用于预设按钮等）。
pub fn settings_indented<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
  let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
  if narrow {
    add_contents(ui)
  } else {
    ui.horizontal(|ui| {
      ui.add_space(theme::SETTINGS_LABEL_WIDTH + 8.0);
      add_contents(ui)
    })
    .inner
  }
}

/// 多列复选框网格，列宽均分。
pub fn checkbox_grid(
  ui: &mut Ui,
  options: &mut [(&mut bool, &str)],
  enabled: bool,
  columns: usize,
) {
  if options.is_empty() {
    return;
  }

  let columns = columns.max(1);
  let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
  let cols = if narrow { 1 } else { columns };

  ui.columns(cols, |columns_ui| {
    for (idx, (value, label)) in options.iter_mut().enumerate() {
      columns_ui[idx % cols].add_enabled(enabled, egui::Checkbox::new(*value, *label));
    }
  });
}

pub fn quality_preset_chip(ui: &mut Ui, label: &str, value: u8, current: &mut u8, enabled: bool) {
  if toggle_chip(ui, label, *current == value, enabled) {
    *current = value;
  }
}

pub fn quality_slider_row(ui: &mut Ui, quality: &mut u8, enabled: bool) {
  settings_labeled_row(ui, &format!("质量  {quality}"), |ui| {
    let slider_w = ui.available_width().max(120.0);
    let slider_h = ui.spacing().interact_size.y;
    ui.add_enabled_ui(enabled, |ui| {
      ui.add_sized(
        egui::vec2(slider_w, slider_h),
        egui::Slider::new(quality, 1..=100).show_value(false),
      );
    });
  });
}

pub fn quality_presets_row(ui: &mut Ui, quality: &mut u8, enabled: bool) {
  settings_indented(ui, |ui| {
    let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
    if narrow {
      ui.horizontal_wrapped(|ui| {
        quality_preset_chip(ui, "Web", 75, quality, enabled);
        ui.add_space(6.0);
        quality_preset_chip(ui, "默认", 85, quality, enabled);
        ui.add_space(6.0);
        quality_preset_chip(ui, "打印", 95, quality, enabled);
      });
    } else {
      ui.horizontal(|ui| {
        quality_preset_chip(ui, "Web", 75, quality, enabled);
        ui.add_space(6.0);
        quality_preset_chip(ui, "默认", 85, quality, enabled);
        ui.add_space(6.0);
        quality_preset_chip(ui, "打印", 95, quality, enabled);
      });
    }
  });
}

pub fn status_banner(ui: &mut Ui, text: &str, running: bool) {
  let dark = ui.style().visuals.dark_mode;
  let (fill, fg) = if running {
    (
      theme::accent(dark).linear_multiply(0.16),
      theme::accent(dark),
    )
  } else {
    (theme::log_fill(dark), theme::secondary_label(dark))
  };

  Frame::new()
    .fill(fill)
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .inner_margin(Margin::symmetric(14, 10))
    .stroke(theme::separator_stroke(dark))
    .show(ui, |ui| {
      ui.set_width(ui.available_width());
      ui.label(RichText::new(text).size(13.5).color(fg));
    });
}

pub fn log_panel(ui: &mut Ui, lines: &[String], max_height: f32) {
  let dark = ui.style().visuals.dark_mode;
  ui.label(
    RichText::new("日志")
      .font(theme::section_header_font())
      .strong()
      .color(theme::secondary_label(dark)),
  );
  ui.add_space(6.0);

  Frame::new()
    .fill(theme::log_fill(dark))
    .corner_radius(CornerRadius::same(theme::GROUP_RADIUS))
    .inner_margin(Margin::symmetric(12, 10))
    .stroke(theme::separator_stroke(dark))
    .show(ui, |ui| {
      ui.set_width(ui.available_width());
      egui::ScrollArea::vertical()
        .max_height(max_height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
          ui.set_width(ui.available_width());
          if lines.is_empty() {
            ui.label(
              RichText::new("转换记录会显示在这里")
                .italics()
                .color(theme::secondary_label(dark)),
            );
          } else {
            for line in lines {
              ui.label(
                RichText::new(line)
                  .font(egui::FontId::monospace(12.0))
                  .color(theme::secondary_label(dark)),
              );
            }
          }
        });
    });
}

pub fn drop_hint(ui: &mut Ui) {
  let dark = ui.style().visuals.dark_mode;
  ui.add_space(4.0);
  ui.label(
    RichText::new("提示：可将文件夹拖入窗口以选择输入目录")
      .size(12.0)
      .italics()
      .color(theme::secondary_label(dark)),
  );
}
