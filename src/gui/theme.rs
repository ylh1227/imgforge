//! ImgForge 主题：macOS 设置式冷灰面 + 系统蓝强调色。
//!
//! 布局遵循 8pt 网格与 inset grouped list，便于工具型界面扫读。

use std::sync::OnceLock;

use eframe::egui::{
    self, epaint::Shadow, style::ScrollStyle, Color32, CornerRadius, FontId, Stroke, Visuals,
};

use crate::gui::macos::{self, AccessibilityPrefs};

/// 控件圆角。
pub const CONTROL_RADIUS: u8 = 10;
/// 分组内容区圆角。
pub const GROUP_RADIUS: u8 = 12;
/// 底部工具栏顶边圆角。
pub const TOOLBAR_TOP_RADIUS: u8 = 10;
/// 状态徽章圆角（略更圆，便于扫读）。
pub const BADGE_RADIUS: u8 = 8;

/// 内容区最大宽度（宽屏居中，不无限拉伸）。
pub const CONTENT_MAX_WIDTH: f32 = 960.0;
/// 内容区最小可用宽度。
pub const CONTENT_MIN_WIDTH: f32 = 320.0;
/// 窄屏断点：控件改为纵向堆叠。
pub const NARROW_BREAKPOINT: f32 = 520.0;
/// 设置表单断点：转换页半栏约 450–480，仍应保持「标签左 + 控件右」，勿套用整页窄屏。
pub const SETTINGS_NARROW_BREAKPOINT: f32 = 360.0;
/// 转换设置区标签列宽（须盖住「目标格式」「目标体积」四字；文案左对齐）。
pub const SETTINGS_LABEL_WIDTH: f32 = 72.0;
/// 设置表单同行间距（字段之间紧凑；分组之间用 separator / collapsing）。
pub const SETTINGS_ROW_GAP: f32 = 4.0;
/// 设置分组标题下到首控件的间距。
pub const SETTINGS_HEADING_GAP: f32 = 4.0;
/// 双栏行内次标签宽（「类型」「认证」「优先级」），须盖住三字中文。
pub const SETTINGS_INLINE_LABEL_WIDTH: f32 = 56.0;

// —— 页面级侧栏+主区自适应（新模块请复用 widgets::side_main_*，勿写死 LEFT_W）——
/// 侧栏+主区并排的通用断点（视频评审 / 同类双栏页）。
pub const SIDE_MAIN_WIDE_BREAKPOINT: f32 = 780.0;
/// 通用侧栏目标宽度。
pub const SIDE_MAIN_LEFT_W: f32 = 280.0;
/// 侧栏与主区间距。
pub const SIDE_MAIN_GAP: f32 = 12.0;
/// 主区右侧留白（滚动条/安全边距）。
pub const SIDE_MAIN_RIGHT_INSET: f32 = 16.0;
/// 纵向滚动条预留宽度，避免控件画进滚动条区域被裁切。
pub const SCROLLBAR_GUTTER: f32 = 18.0;
/// 窄屏堆叠时侧栏最大高度占比。
pub const SIDE_MAIN_STACK_SIDE_FRAC: f32 = 0.42;
/// 窄屏堆叠时侧栏高度上下限。
pub const SIDE_MAIN_STACK_SIDE_MIN_H: f32 = 220.0;
pub const SIDE_MAIN_STACK_SIDE_MAX_H: f32 = 360.0;

/// 评审三栏布局最小宽度（左+中+右+边距，不足则降级）。
pub const REVIEW_THREE_COL_BREAKPOINT: f32 = 1180.0;
/// 评审双栏最小宽度；低于此值直接分段堆叠（避免小窗双栏挤压）。
pub const REVIEW_TWO_COL_BREAKPOINT: f32 = 980.0;
/// 评审左侧栏目标宽度。
pub const REVIEW_LEFT_W: f32 = 280.0;
/// 评审右侧栏目标宽度。
pub const REVIEW_RIGHT_W: f32 = 280.0;
/// 评审画布区最小宽度。
pub const REVIEW_CENTER_MIN_W: f32 = 280.0;
/// 三栏正文区最小高度（低于此值改为整页滚动）。
pub const REVIEW_MIN_BODY_H: f32 = 320.0;
/// 数据提取侧栏+表格并排最小宽度。
pub const DATA_EXTRACT_WIDE_BREAKPOINT: f32 = SIDE_MAIN_WIDE_BREAKPOINT;
/// 数据提取左侧栏目标宽度。
pub const DATA_EXTRACT_LEFT_W: f32 = 272.0;
/// 视频评审侧栏+播放区并排最小宽度。
pub const VIDEO_REVIEW_WIDE_BREAKPOINT: f32 = SIDE_MAIN_WIDE_BREAKPOINT;
/// 视频评审左侧栏目标宽度。
pub const VIDEO_REVIEW_LEFT_W: f32 = SIDE_MAIN_LEFT_W;
/// 日志面板高度上下限。
pub const LOG_MIN_HEIGHT: f32 = 96.0;
pub const LOG_MAX_HEIGHT: f32 = 360.0;

/// 页面区块间距（8pt 网格）。
pub const SECTION_GAP: f32 = 12.0;
/// 页头（副标题）与首个区块间距。
pub const PAGE_HEADER_GAP: f32 = 8.0;
/// 顶栏 Tab 与内容区间距。
pub const CHROME_GAP: f32 = 8.0;
/// 转换页宽屏双栏断点。
pub const CONVERT_WIDE_BREAKPOINT: f32 = 880.0;
/// 转换页双栏等分（对称布局）。
pub const CONVERT_PRIMARY_FRAC: f32 = 0.5;

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

/// 成功 / 通过。
pub fn success_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(72, 200, 120)
    } else {
        Color32::from_rgb(36, 160, 80)
    }
}

/// 警告 / 需关注。
pub fn warning_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(255, 170, 60)
    } else {
        Color32::from_rgb(210, 130, 20)
    }
}

/// 信息 / 中性提示。
pub fn info_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(140, 170, 200)
    } else {
        Color32::from_rgb(70, 100, 130)
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

pub fn error_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(255, 105, 97)
    } else {
        Color32::from_rgb(255, 59, 48)
    }
}

/// 窗口 / 内容区背景。
pub fn window_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(22, 22, 24)
    } else {
        Color32::from_rgb(242, 242, 247)
    }
}

/// 分组列表背景（inset card）。
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

/// 输入框、次要按钮等控件背景。
pub fn control_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(50, 50, 52)
    } else {
        Color32::from_rgb(255, 255, 255)
    }
}

pub fn control_stroke(dark: bool) -> Stroke {
    let alpha = if a11y().increase_contrast { 100 } else { 55 };
    let c = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, alpha)
    } else {
        Color32::from_rgba_unmultiplied(60, 60, 67, alpha)
    };
    Stroke::new(1.0, c)
}

/// 底部操作栏背景。
pub fn toolbar_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(36, 36, 38)
    } else {
        Color32::from_rgb(251, 251, 253)
    }
}

/// 顶部分段切换轨道背景。
pub fn segment_track_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(50, 50, 52)
    } else {
        Color32::from_rgb(229, 229, 234)
    }
}

/// 兼容旧名；与 [`control_fill`] 相同。
pub fn glass_regular(dark: bool) -> Color32 {
    control_fill(dark)
}

/// 兼容旧名；与 [`control_stroke`] 相同。
pub fn glass_stroke(dark: bool) -> Stroke {
    control_stroke(dark)
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

pub fn toolbar_shadow(_dark: bool) -> Shadow {
    Shadow::NONE
}

/// 应用主题视觉（浅色/深色随系统；尊重辅助功能偏好）。
pub fn apply(ctx: &egui::Context) {
    let _ = ACCESSIBILITY.set(macos::accessibility_prefs());
    let dark = ctx.style().visuals.dark_mode;
    let mut style = (*ctx.style()).clone();

    // 8pt 网格
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(18.0, 10.0);
    style.spacing.indent = 20.0;
    style.spacing.window_margin = egui::Margin::symmetric(20, 16);
    style.spacing.slider_width = 180.0;
    // 默认 floating 滚动条不占位、叠在内容上 → 右侧控件被裁切。
    // 改为实心滚动条，始终为内容留出宽度。
    style.spacing.scroll = ScrollStyle::solid();

    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

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

    visuals.widgets.inactive.bg_fill = control_fill(dark);
    visuals.widgets.inactive.fg_stroke.color = primary_label(dark);
    visuals.widgets.inactive.bg_stroke = control_stroke(dark);

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
    FontId::proportional(30.0)
}

pub fn subtitle_font() -> FontId {
    FontId::proportional(14.0)
}

pub fn section_font() -> FontId {
    FontId::proportional(13.0)
}

pub fn section_header_font() -> FontId {
    FontId::proportional(11.5)
}

pub fn badge_font() -> FontId {
    FontId::proportional(11.0)
}

/// macOS 全尺寸标题栏内容区顶部留白。
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
    (viewport_width - inset * 2.0).clamp(CONTENT_MIN_WIDTH, CONTENT_MAX_WIDTH)
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
