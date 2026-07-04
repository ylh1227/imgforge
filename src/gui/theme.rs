//! 平台主题：macOS 26 风格分组卡片、系统色与圆角。

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, Visuals};

/// macOS 系统蓝（浅色 / 深色）。
pub fn accent(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(10, 132, 255)
  } else {
    Color32::from_rgb(0, 122, 255)
  }
}

pub fn secondary_label(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(152, 152, 157)
  } else {
    Color32::from_rgb(110, 110, 115)
  }
}

pub fn card_fill(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(44, 44, 46)
  } else {
    Color32::from_rgb(255, 255, 255)
  }
}

pub fn window_fill(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(28, 28, 30)
  } else {
    Color32::from_rgb(242, 242, 247)
  }
}

pub fn log_fill(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(36, 36, 38)
  } else {
    Color32::from_rgb(248, 248, 250)
  }
}

pub fn separator_stroke(dark: bool) -> Stroke {
  let c = if dark {
    Color32::from_rgba_unmultiplied(255, 255, 255, 28)
  } else {
    Color32::from_rgba_unmultiplied(60, 60, 67, 30)
  };
  Stroke::new(1.0, c)
}

/// 应用 macOS 风格视觉（浅色/深色随系统）。
pub fn apply(ctx: &egui::Context) {
  let dark = ctx.style().visuals.dark_mode;
  let mut style = (*ctx.style()).clone();

  style.spacing.item_spacing = egui::vec2(10.0, 10.0);
  style.spacing.button_padding = egui::vec2(16.0, 9.0);
  style.spacing.indent = 18.0;
  style.spacing.window_margin = egui::Margin::symmetric(20, 18);

  let mut visuals = if dark {
    Visuals::dark()
  } else {
    Visuals::light()
  };

  let accent = accent(dark);
  let card = card_fill(dark);
  let window = window_fill(dark);
  let radius = CornerRadius::same(12);

  visuals.window_fill = window;
  visuals.panel_fill = window;
  visuals.faint_bg_color = log_fill(dark);
  visuals.extreme_bg_color = card;
  visuals.window_corner_radius = CornerRadius::same(14);
  visuals.widgets.noninteractive.corner_radius = radius;
  visuals.widgets.inactive.corner_radius = radius;
  visuals.widgets.hovered.corner_radius = radius;
  visuals.widgets.active.corner_radius = radius;
  visuals.widgets.open.corner_radius = radius;

  visuals.selection.bg_fill = accent.linear_multiply(0.28);
  visuals.selection.stroke = Stroke::new(1.0, accent);
  visuals.hyperlink_color = accent;

  visuals.widgets.inactive.bg_fill = card;
  visuals.widgets.inactive.fg_stroke.color = if dark {
    Color32::from_rgb(235, 235, 245)
  } else {
    Color32::from_rgb(28, 28, 30)
  };
  visuals.widgets.inactive.bg_stroke = separator_stroke(dark);

  visuals.widgets.hovered.bg_fill = if dark {
    Color32::from_rgb(58, 58, 60)
  } else {
    Color32::from_rgb(245, 245, 247)
  };

  visuals.widgets.active.bg_fill = accent.linear_multiply(0.85);
  visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

  style.visuals = visuals;
  ctx.set_style(style);
}

pub fn title_font() -> FontId {
  FontId::proportional(28.0)
}

pub fn subtitle_font() -> FontId {
  FontId::proportional(14.0)
}

pub fn section_font() -> FontId {
  FontId::proportional(13.0)
}
