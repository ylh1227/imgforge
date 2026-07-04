//! 平台主题：遵循 macOS 26 Liquid Glass HIG——
//! 导航/控件层使用半透明材质，内容层保持扁平、少自定义背景。

use std::sync::OnceLock;

use eframe::egui::{self, epaint::Shadow, Color32, CornerRadius, FontId, Stroke, Visuals};

use crate::gui::macos::{self, AccessibilityPrefs};

/// 控件圆角（macOS 26 控件更圆润，参考 NSGlassEffectView 16pt）。
pub const CONTROL_RADIUS: u8 = 16;
/// 分组内容区圆角。
pub const GROUP_RADIUS: u8 = 16;
/// 底部工具栏圆角（仅顶边）。
pub const TOOLBAR_TOP_RADIUS: u8 = 20;

/// 内容区最大宽度（宽屏居中，不无限拉伸）。
pub const CONTENT_MAX_WIDTH: f32 = 960.0;
/// 内容区最小可用宽度。
pub const CONTENT_MIN_WIDTH: f32 = 320.0;
/// 窄屏断点：控件改为纵向堆叠。
pub const NARROW_BREAKPOINT: f32 = 520.0;
/// 日志面板高度上下限。
pub const LOG_MIN_HEIGHT: f32 = 96.0;
pub const LOG_MAX_HEIGHT: f32 = 360.0;

static ACCESSIBILITY: OnceLock<AccessibilityPrefs> = OnceLock::new();

fn a11y() -> AccessibilityPrefs {
  *ACCESSIBILITY.get_or_init(macos::accessibility_prefs)
}

/// macOS 系统蓝（浅色 / 深色 / 高对比）。
pub fn accent(dark: bool) -> Color32 {
  let high = a11y().increase_contrast;
  if dark {
    if high {
      Color32::from_rgb(64, 156, 255)
    } else {
      Color32::from_rgb(10, 132, 255)
    }
  } else if high {
    Color32::from_rgb(0, 64, 221)
  } else {
    Color32::from_rgb(0, 122, 255)
  }
}

pub fn secondary_label(dark: bool) -> Color32 {
  let high = a11y().increase_contrast;
  if dark {
    if high {
      Color32::from_rgb(200, 200, 205)
    } else {
      Color32::from_rgb(152, 152, 157)
    }
  } else if high {
    Color32::from_rgb(72, 72, 74)
  } else {
    Color32::from_rgb(110, 110, 115)
  }
}

pub fn primary_label(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(245, 245, 247)
  } else {
    Color32::from_rgb(28, 28, 30)
  }
}

/// 窗口 / 内容区背景（非玻璃层）。
pub fn window_fill(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(22, 22, 24)
  } else {
    Color32::from_rgb(242, 242, 247)
  }
}

/// 分组列表背景（内容层，不用 Liquid Glass）。
pub fn grouped_fill(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(44, 44, 46)
  } else {
    Color32::from_rgb(255, 255, 255)
  }
}

pub fn log_fill(dark: bool) -> Color32 {
  if dark {
    Color32::from_rgb(32, 32, 34)
  } else {
    Color32::from_rgb(250, 250, 252)
  }
}

/// Liquid Glass Regular 变体：仅用于工具栏、次要按钮等导航/控件层。
pub fn glass_regular(dark: bool) -> Color32 {
  if a11y().reduce_transparency {
    return if dark {
      Color32::from_rgb(38, 38, 40)
    } else {
      Color32::from_rgb(252, 252, 254)
    };
  }

  if dark {
    Color32::from_rgba_unmultiplied(58, 58, 60, 210)
  } else {
    Color32::from_rgba_unmultiplied(255, 255, 255, 215)
  }
}

pub fn glass_stroke(dark: bool) -> Stroke {
  let alpha = if a11y().increase_contrast { 90 } else { 40 };
  let c = if dark {
    Color32::from_rgba_unmultiplied(255, 255, 255, alpha)
  } else {
    Color32::from_rgba_unmultiplied(60, 60, 67, alpha)
  };
  Stroke::new(1.0, c)
}

pub fn separator_stroke(dark: bool) -> Stroke {
  let alpha = if a11y().increase_contrast { 70 } else { 30 };
  let c = if dark {
    Color32::from_rgba_unmultiplied(255, 255, 255, alpha)
  } else {
    Color32::from_rgba_unmultiplied(60, 60, 67, alpha)
  };
  Stroke::new(1.0, c)
}

pub fn toolbar_shadow(dark: bool) -> Shadow {
  Shadow {
    offset: [0, -2],
    blur: 20,
    spread: 0,
    color: if dark {
      Color32::from_black_alpha(100)
    } else {
      Color32::from_black_alpha(28)
    },
  }
}

/// 应用 macOS 26 风格视觉（浅色/深色随系统；尊重辅助功能偏好）。
pub fn apply(ctx: &egui::Context) {
  let _ = ACCESSIBILITY.set(macos::accessibility_prefs());
  let dark = ctx.style().visuals.dark_mode;
  let mut style = (*ctx.style()).clone();

  // Apple 8pt 网格间距
  style.spacing.item_spacing = egui::vec2(8.0, 8.0);
  style.spacing.button_padding = egui::vec2(18.0, 10.0);
  style.spacing.indent = 20.0;
  style.spacing.window_margin = egui::Margin::symmetric(20, 16);
  style.spacing.slider_width = 180.0;

  let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };

  let accent_color = accent(dark);
  let window = window_fill(dark);
  let grouped = grouped_fill(dark);
  let radius = CornerRadius::same(CONTROL_RADIUS);

  visuals.window_fill = window;
  visuals.panel_fill = window;
  visuals.faint_bg_color = log_fill(dark);
  visuals.extreme_bg_color = grouped;
  visuals.window_corner_radius = CornerRadius::same(TOOLBAR_TOP_RADIUS);
  visuals.widgets.noninteractive.corner_radius = radius;
  visuals.widgets.inactive.corner_radius = radius;
  visuals.widgets.hovered.corner_radius = radius;
  visuals.widgets.active.corner_radius = radius;
  visuals.widgets.open.corner_radius = radius;

  visuals.selection.bg_fill = accent_color.linear_multiply(0.28);
  visuals.selection.stroke = Stroke::new(1.5, accent_color);
  visuals.hyperlink_color = accent_color;

  visuals.widgets.inactive.bg_fill = glass_regular(dark);
  visuals.widgets.inactive.fg_stroke.color = primary_label(dark);
  visuals.widgets.inactive.bg_stroke = glass_stroke(dark);

  visuals.widgets.hovered.bg_fill = if dark {
    Color32::from_rgb(72, 72, 74)
  } else {
    Color32::from_rgb(248, 248, 250)
  };

  visuals.widgets.active.bg_fill = accent_color.linear_multiply(0.88);
  visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

  style.visuals = visuals;
  ctx.set_style(style);
}

pub fn title_font() -> FontId {
  FontId::proportional(34.0)
}

pub fn subtitle_font() -> FontId {
  FontId::proportional(15.0)
}

pub fn section_font() -> FontId {
  FontId::proportional(13.0)
}

pub fn section_header_font() -> FontId {
  FontId::proportional(12.0)
}

/// macOS 全尺寸标题栏内容区顶部留白（Liquid Glass 内容不侵入标题栏控件区）。
pub fn macos_titlebar_inset(ctx: &egui::Context) -> f32 {
  #[cfg(target_os = "macos")]
  {
    let _ = ctx;
    36.0
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = ctx;
    0.0
  }
}

/// 视口尺寸（逻辑像素）。
pub fn viewport_size(ctx: &egui::Context) -> egui::Vec2 {
  ctx.input(|input| {
    input
      .viewport()
      .inner_rect
      .map(|rect| rect.size())
      .unwrap_or_else(|| ctx.screen_rect().size())
  })
}

/// 内容区左右内边距（随窗口宽度缩放）。
pub fn content_side_inset(viewport_width: f32) -> f32 {
  if viewport_width < 560.0 {
    12.0
  } else if viewport_width < 800.0 {
    16.0
  } else {
    24.0
  }
}

/// 主内容区可用宽度（随窗口缩放，宽屏封顶居中）。
pub fn content_width(viewport_width: f32) -> f32 {
  let inset = content_side_inset(viewport_width);
  (viewport_width - inset * 2.0)
    .clamp(CONTENT_MIN_WIDTH, CONTENT_MAX_WIDTH)
}

/// 日志面板高度：窗口变高时扩展，变矮时收缩。
pub fn log_panel_height(viewport_height: f32, bottom_reserve: f32) -> f32 {
  let fixed_estimate = 520.0 + macos_titlebar_inset_unconditional();
  let flexible = viewport_height - fixed_estimate - bottom_reserve;
  flexible.clamp(LOG_MIN_HEIGHT, LOG_MAX_HEIGHT)
}

fn macos_titlebar_inset_unconditional() -> f32 {
  #[cfg(target_os = "macos")]
  {
    36.0
  }
  #[cfg(not(target_os = "macos"))]
  {
    0.0
  }
}
