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

/// 底部操作工具栏帧（扁平实色底栏 + 顶部分割线）。
pub fn glass_toolbar_frame(dark: bool) -> Frame {
  Frame::new()
    .fill(theme::toolbar_fill(dark))
    .stroke(theme::separator_stroke(dark))
    .shadow(theme::toolbar_shadow(dark))
    .inner_margin(Margin::symmetric(20, 12))
    .corner_radius(CornerRadius::same(theme::TOOLBAR_TOP_RADIUS))
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

pub fn quality_preset_chip(ui: &mut Ui, label: &str, value: u8, current: &mut u8, enabled: bool) {
  if toggle_chip(ui, label, *current == value, enabled) {
    *current = value;
  }
}

pub fn quality_slider_row(ui: &mut Ui, quality: &mut u8, enabled: bool) {
  let dark = ui.style().visuals.dark_mode;
  let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;

  if narrow {
    ui.label(
      RichText::new(format!("质量  {}", quality))
        .font(theme::section_font())
        .color(theme::primary_label(dark)),
    );
    ui.add_space(4.0);
    let slider_w = ui.available_width().max(120.0);
    let slider_h = ui.spacing().interact_size.y;
    ui.add_enabled_ui(enabled, |ui| {
      ui.add_sized(
        egui::vec2(slider_w, slider_h),
        egui::Slider::new(quality, 1..=100).show_value(false),
      );
    });
  } else {
    ui.horizontal(|ui| {
      ui.label(
        RichText::new(format!("质量  {}", quality))
          .font(theme::section_font())
          .color(theme::primary_label(dark)),
      );
      ui.add_space(8.0);
      let slider_w = (ui.available_width() - 8.0).max(120.0);
      let slider_h = ui.spacing().interact_size.y;
      ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(
          egui::vec2(slider_w, slider_h),
          egui::Slider::new(quality, 1..=100).show_value(false),
        );
      });
    });
  }
}

pub fn quality_presets_row(ui: &mut Ui, quality: &mut u8, enabled: bool) {
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
