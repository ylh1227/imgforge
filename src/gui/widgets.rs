//! 可复用 UI 组件（macOS 26 Liquid Glass 布局）。

use eframe::egui::{self, Button, Color32, CornerRadius, Frame, Margin, RichText, Stroke, TextEdit, Ui};

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

/// 内容层分组（inset grouped list），不使用 Liquid Glass。
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
    .show(ui, |ui| add_contents(ui))
    .inner
}

/// 底部操作工具栏帧（Liquid Glass Regular 模拟层）。
pub fn glass_toolbar_frame(dark: bool) -> Frame {
  Frame::new()
    .fill(theme::glass_regular(dark))
    .stroke(theme::glass_stroke(dark))
    .shadow(theme::toolbar_shadow(dark))
    .inner_margin(Margin::symmetric(20, 14))
    .corner_radius(CornerRadius {
      nw: theme::TOOLBAR_TOP_RADIUS,
      ne: theme::TOOLBAR_TOP_RADIUS,
      sw: 0,
      se: 0,
    })
}

pub fn folder_field(ui: &mut Ui, label: &str, path: &mut String, enabled: bool) {
  let dark = ui.style().visuals.dark_mode;
  ui.horizontal(|ui| {
    ui.label(
      RichText::new(label)
        .font(theme::section_font())
        .color(theme::primary_label(dark)),
    );
    ui.add_space(12.0);
    let edit = TextEdit::singleline(path)
      .hint_text("选择或拖入文件夹…")
      .margin(egui::vec2(12.0, 10.0))
      .desired_width(ui.available_width() - 100.0);
    ui.add_enabled(enabled, edit);
    if ui
      .add_enabled(
        enabled,
        Button::new(RichText::new("浏览…").size(13.0))
          .fill(theme::glass_regular(dark))
          .stroke(theme::glass_stroke(dark))
          .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS)),
      )
      .clicked()
    {
      if let Some(folder) = rfd::FileDialog::new().pick_folder() {
        *path = folder.display().to_string();
      }
    }
  });
  ui.add_space(4.0);
  ui.separator();
  ui.add_space(4.0);
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
    .fill(theme::glass_regular(dark))
    .stroke(theme::glass_stroke(dark))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(96.0, 38.0));
  ui.add_enabled(enabled, btn)
}

pub fn quality_preset_chip(ui: &mut Ui, label: &str, value: u8, current: &mut u8, enabled: bool) {
  let dark = ui.style().visuals.dark_mode;
  let selected = *current == value;
  let accent = theme::accent(dark);
  let (fill, stroke, fg) = if selected {
    (
      accent.linear_multiply(0.22),
      Stroke::new(1.5, accent),
      accent,
    )
  } else {
    (
      theme::glass_regular(dark),
      theme::glass_stroke(dark),
      theme::primary_label(dark),
    )
  };

  let btn = Button::new(RichText::new(label).size(13.0).color(fg))
    .fill(fill)
    .stroke(stroke)
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(56.0, 32.0));

  if ui.add_enabled(enabled, btn).clicked() {
    *current = value;
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
      ui.label(RichText::new(text).size(13.5).color(fg));
    });
}

pub fn log_panel(ui: &mut Ui, lines: &[String]) {
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
      egui::ScrollArea::vertical()
        .max_height(160.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
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
