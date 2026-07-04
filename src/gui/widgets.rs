//! 可复用 UI 组件。

use eframe::egui::{self, Button, Color32, CornerRadius, Frame, Margin, RichText, TextEdit, Ui};

use crate::gui::theme;

pub fn header(ui: &mut Ui, subtitle: &str) {
  let dark = ui.style().visuals.dark_mode;
  ui.vertical(|ui| {
    ui.label(
      RichText::new("ImgForge")
        .font(theme::title_font())
        .strong()
        .color(if dark {
          Color32::from_rgb(245, 245, 247)
        } else {
          Color32::from_rgb(28, 28, 30)
        }),
    );
    ui.add_space(2.0);
    ui.label(
      RichText::new(subtitle)
        .font(theme::subtitle_font())
        .color(theme::secondary_label(dark)),
    );
  });
}

pub fn section_card<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
  let dark = ui.style().visuals.dark_mode;
  Frame::new()
    .fill(theme::card_fill(dark))
    .corner_radius(CornerRadius::same(14))
    .inner_margin(Margin::symmetric(18, 16))
    .stroke(theme::separator_stroke(dark))
    .show(ui, |ui| {
      ui.label(
        RichText::new(title)
          .font(theme::section_font())
          .strong()
          .color(theme::secondary_label(dark)),
      );
      ui.add_space(12.0);
      add_contents(ui)
    })
    .inner
}

pub fn folder_field(ui: &mut Ui, label: &str, path: &mut String, enabled: bool) {
  let dark = ui.style().visuals.dark_mode;
  ui.vertical(|ui| {
    ui.label(
      RichText::new(label)
        .font(theme::section_font())
        .color(if dark {
          Color32::from_rgb(220, 220, 225)
        } else {
          Color32::from_rgb(45, 45, 48)
        }),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
      let edit = TextEdit::singleline(path)
        .hint_text("选择或拖入文件夹…")
        .margin(egui::vec2(10.0, 8.0))
        .desired_width(ui.available_width() - 96.0);
      ui.add_enabled(enabled, edit);
      if ui
        .add_enabled(enabled, Button::new(RichText::new("浏览…").size(13.0)))
        .clicked()
      {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
          *path = folder.display().to_string();
        }
      }
    });
  });
  ui.add_space(6.0);
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
    .corner_radius(CornerRadius::same(10))
    .min_size(egui::vec2(132.0, 36.0));
  ui.add_enabled(enabled, btn)
}

pub fn secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
  let dark = ui.style().visuals.dark_mode;
  let btn = Button::new(RichText::new(label).size(14.0))
    .fill(theme::card_fill(dark))
    .stroke(theme::separator_stroke(dark))
    .corner_radius(CornerRadius::same(10))
    .min_size(egui::vec2(88.0, 36.0));
  ui.add_enabled(enabled, btn)
}

pub fn status_banner(ui: &mut Ui, text: &str, running: bool) {
  let dark = ui.style().visuals.dark_mode;
  let (fill, fg) = if running {
    (
      theme::accent(dark).linear_multiply(0.18),
      theme::accent(dark),
    )
  } else {
    (theme::log_fill(dark), theme::secondary_label(dark))
  };

  Frame::new()
    .fill(fill)
    .corner_radius(CornerRadius::same(10))
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
      .font(theme::section_font())
      .strong()
      .color(theme::secondary_label(dark)),
  );
  ui.add_space(6.0);

  Frame::new()
    .fill(theme::log_fill(dark))
    .corner_radius(CornerRadius::same(12))
    .inner_margin(Margin::symmetric(12, 10))
    .stroke(theme::separator_stroke(dark))
    .show(ui, |ui| {
      egui::ScrollArea::vertical()
        .max_height(150.0)
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
                  .color(if dark {
                    Color32::from_rgb(200, 200, 205)
                  } else {
                    Color32::from_rgb(55, 55, 60)
                  }),
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
