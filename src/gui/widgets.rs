//! 可复用 UI 组件（macOS 分组列表布局，随窗口自适应）。

use eframe::egui::{self, Button, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke, TextEdit, Ui};

use crate::gui::theme;

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

/// 内容层分组（inset grouped list），宽度随父级拉伸。
pub fn grouped_section<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
  let dark = ui.style().visuals.dark_mode;
  ui.label(
    RichText::new(title)
      .font(theme::section_header_font())
      .strong()
      .color(theme::secondary_label(dark)),
  );
  ui.add_space(6.0);

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

/// 工具栏用紧凑次要按钮（评审操作栏等）。
pub fn compact_secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
  let dark = ui.style().visuals.dark_mode;
  let btn = Button::new(RichText::new(label).size(13.0).color(theme::primary_label(dark)))
    .fill(theme::control_fill(dark))
    .stroke(theme::control_stroke(dark))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(72.0, 32.0));
  ui.add_enabled(enabled, btn)
}

/// 工具栏用紧凑主要按钮。
pub fn compact_primary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
  let dark = ui.style().visuals.dark_mode;
  let accent = theme::accent(dark);
  let btn = Button::new(RichText::new(label).size(13.0).strong().color(Color32::WHITE))
    .fill(if enabled {
      accent
    } else {
      accent.linear_multiply(0.45)
    })
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(88.0, 32.0));
  ui.add_enabled(enabled, btn)
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
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(56.0, 32.0));

  ui.add_enabled(enabled, btn).clicked()
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
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(56.0, 32.0));
  ui.add_enabled(enabled, btn).clicked()
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
