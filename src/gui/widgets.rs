//! 可复用 UI 组件（macOS 分组列表布局，随窗口自适应）。

use eframe::egui::{
    self, Button, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke, TextEdit, Ui,
};

use crate::gui::theme;

/// 工具栏统一行高（与 compact 按钮、状态芯片一致）。
pub const TOOLBAR_ROW_HEIGHT: f32 = 32.0;

/// 等分列宽：保证 `cols * cell + (cols-1)*gap <= total`，不会因下限撑破容器。
pub fn equal_cell_width(total: f32, gap: f32, cols: usize) -> f32 {
    let cols = cols.max(1) as f32;
    let usable = (total - gap * (cols - 1.0)).max(cols);
    (usable / cols).floor().max(1.0)
}

/// 侧栏 + 主区布局模式（各业务页共用，防止小窗裁切侧栏）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideMainMode {
    SideBySide,
    Stacked,
}

impl SideMainMode {
    pub fn from_width(avail_w: f32, breakpoint: f32) -> Self {
        if avail_w >= breakpoint {
            Self::SideBySide
        } else {
            Self::Stacked
        }
    }
}

/// 侧栏 + 主区几何参数。新模块按此分配宽度，勿再写死 `LEFT_W` 后让主区贪婪扩张。
#[derive(Debug, Clone, Copy)]
pub struct SideMainGeometry {
    pub mode: SideMainMode,
    pub left_w: f32,
    pub main_w: f32,
    pub row_h: f32,
    pub side_max_h: f32,
    pub gap: f32,
    pub right_inset: f32,
}

impl SideMainGeometry {
    /// 根据可用区域与断点计算布局；`left_w` 为宽屏侧栏目标宽。
    pub fn compute(avail: egui::Vec2, breakpoint: f32, left_w: f32) -> Self {
        let gap = theme::SIDE_MAIN_GAP;
        let right_inset = theme::SIDE_MAIN_RIGHT_INSET;
        let mode = SideMainMode::from_width(avail.x, breakpoint);
        let side_max_h = (avail.y * theme::SIDE_MAIN_STACK_SIDE_FRAC)
            .clamp(theme::SIDE_MAIN_STACK_SIDE_MIN_H, theme::SIDE_MAIN_STACK_SIDE_MAX_H);
        match mode {
            SideMainMode::SideBySide => {
                let main_w = (avail.x - left_w - gap - right_inset).max(160.0);
                Self {
                    mode,
                    left_w,
                    main_w,
                    row_h: avail.y.max(280.0),
                    side_max_h,
                    gap,
                    right_inset,
                }
            }
            SideMainMode::Stacked => Self {
                mode,
                left_w: avail.x,
                main_w: avail.x,
                row_h: avail.y.max(280.0),
                side_max_h,
                gap,
                right_inset,
            },
        }
    }
}

/// 工具栏内按钮内边距（小于全局 `button_padding`，以便固定行高内垂直居中）。
const TOOLBAR_BUTTON_PADDING: egui::Vec2 = egui::vec2(10.0, 4.0);
/// 与 compact 按钮描边对齐，纯文本标签需补一点左距。
const TOOLBAR_STROKE_INSET: f32 = 2.0;

fn add_toolbar_sized_button(
    ui: &mut Ui,
    size: egui::Vec2,
    enabled: bool,
    btn: Button,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| ui.add_sized(size, btn))
        .inner
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
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(1.0, TOOLBAR_ROW_HEIGHT), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        theme::separator_stroke(dark),
    );
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
    let accent = theme::accent(dark);
    ui.vertical(|ui| {
        let brand = ui.label(
            RichText::new("ImgForge")
                .font(theme::title_font())
                .strong()
                .color(theme::primary_label(dark)),
        );
        // 签名：品牌字下方细强调色线
        let underline = egui::Rect::from_min_size(
            egui::pos2(brand.rect.left(), brand.rect.bottom() + 2.0),
            egui::vec2((brand.rect.width() * 0.42).clamp(36.0, 88.0), 2.5),
        );
        ui.painter()
            .rect_filled(underline, CornerRadius::same(2), accent);
        ui.add_space(8.0);
        ui.label(
            RichText::new(subtitle)
                .font(theme::subtitle_font())
                .color(theme::secondary_label(dark)),
        );
    });
}

/// 页头之后的标准间距。
pub fn page_header_gap(ui: &mut Ui) {
    ui.add_space(theme::PAGE_HEADER_GAP);
}

/// 区块之间的标准间距。
pub fn section_gap(ui: &mut Ui) {
    ui.add_space(theme::SECTION_GAP);
}

/// 顶栏与内容区间距。
pub fn chrome_gap(ui: &mut Ui) {
    ui.add_space(theme::CHROME_GAP);
}

/// 居中内容列：按视口宽度封顶，左右留白一致。
pub fn content_column<R>(
    ui: &mut Ui,
    content_width: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.vertical_centered(|ui| {
        ui.set_width(content_width);
        add_contents(ui)
    })
    .inner
}

/// 分组标题（左侧细强调色条 + 小号字重）。
pub fn section_header(ui: &mut Ui, title: &str) {
    let dark = ui.style().visuals.dark_mode;
    let accent = theme::accent(dark);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (bar_rect, _) =
            ui.allocate_exact_size(egui::vec2(3.0, 12.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(bar_rect, CornerRadius::same(2), accent);
        ui.label(
            RichText::new(title)
                .font(theme::section_header_font())
                .strong()
                .color(theme::secondary_label(dark)),
        );
    });
}

/// 分组内容框（无标题），始终拉满父级可用宽度。
pub fn grouped_section_frame<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let dark = ui.style().visuals.dark_mode;
    // 锁定在可视 max_rect 内，避免 set_min_width 把父级撑出裁切区
    let outer_w = ui
        .available_width()
        .min(ui.max_rect().width())
        .max(80.0);
    ui.set_max_width(outer_w);
    ui.set_width(outer_w);

    Frame::new()
        .fill(theme::grouped_fill(dark))
        .stroke(theme::separator_stroke(dark))
        .corner_radius(CornerRadius::same(theme::GROUP_RADIUS))
        .inner_margin(Margin::symmetric(12, 12))
        .show(ui, |ui| {
            let inner_w = ui
                .available_width()
                .min(ui.max_rect().width())
                .max(60.0);
            ui.set_max_width(inner_w);
            ui.set_width(inner_w);
            add_contents(ui)
        })
        .inner
}

/// 拉满当前行宽的主要按钮（侧栏操作区用）。
pub fn full_width_primary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    let w = ui
        .available_width()
        .min(ui.max_rect().width())
        .max(40.0);
    full_width_primary_button_in(ui, label, enabled, w)
}

/// 指定宽度的主要按钮。
pub fn full_width_primary_button_in(
    ui: &mut Ui,
    label: &str,
    enabled: bool,
    width: f32,
) -> egui::Response {
    let dark = ui.style().visuals.dark_mode;
    let accent = theme::accent(dark);
    let btn = Button::new(
        RichText::new(label)
            .size(13.0)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(if enabled {
        accent
    } else {
        accent.linear_multiply(0.45)
    })
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));
    // 必须 add_sized：min_size 会因文案撑破列宽导致右侧裁切
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(egui::vec2(width.max(40.0), TOOLBAR_ROW_HEIGHT), btn)
    })
    .inner
}

/// 拉满当前行宽的次要按钮（侧栏操作区用）。
pub fn full_width_secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    let w = ui
        .available_width()
        .min(ui.max_rect().width())
        .max(40.0);
    full_width_secondary_button_in(ui, label, enabled, w)
}

/// 指定宽度的次要按钮。
pub fn full_width_secondary_button_in(
    ui: &mut Ui,
    label: &str,
    enabled: bool,
    width: f32,
) -> egui::Response {
    let dark = ui.style().visuals.dark_mode;
    let btn = Button::new(
        RichText::new(label)
            .size(13.0)
            .color(theme::primary_label(dark)),
    )
    .fill(theme::control_fill(dark))
    .stroke(theme::control_stroke(dark))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS));
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(egui::vec2(width.max(40.0), TOOLBAR_ROW_HEIGHT), btn)
    })
    .inner
}

/// 空状态：标题 + 一句指引（产品页惯例）。
pub fn empty_state(ui: &mut Ui, headline: &str, detail: &str) {
    let dark = ui.style().visuals.dark_mode;
    Frame::new()
        .fill(theme::log_fill(dark))
        .stroke(theme::separator_stroke(dark))
        .corner_radius(CornerRadius::same(theme::GROUP_RADIUS))
        .inner_margin(Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(headline)
                    .size(14.0)
                    .strong()
                    .color(theme::primary_label(dark)),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(detail)
                    .size(13.0)
                    .color(theme::secondary_label(dark)),
            );
        });
}

/// 状态徽章（成功 / 警告 / 失败 / 信息）。
pub fn status_badge(ui: &mut Ui, label: &str, color: Color32) {
    Frame::new()
        .fill(color.linear_multiply(0.18))
        .stroke(Stroke::new(1.0, color.linear_multiply(0.7)))
        .corner_radius(CornerRadius::same(theme::BADGE_RADIUS))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(label)
                    .font(theme::badge_font())
                    .strong()
                    .color(color),
            );
        });
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
        .inner_margin(Margin::symmetric(16, 12))
        .corner_radius(CornerRadius::ZERO)
}

/// egui 回退工具栏点击结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarClick {
    Start,
    Cancel,
    OpenOutput,
}

/// egui 回退工具栏：整组按钮相对底栏水平居中；两侧等宽、中间主按钮。
pub fn action_toolbar_row(ui: &mut Ui, enabled: bool, running: bool) -> Option<ToolbarClick> {
    let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
    let mut clicked = None;

    const SIDE_W: f32 = 120.0;
    const PRIMARY_W: f32 = 140.0;
    const GAP: f32 = 12.0;
    const BAR_H: f32 = 46.0;

    ui.set_width(ui.available_width());
    ui.set_min_height(BAR_H);

    if narrow {
        ui.with_layout(Layout::top_down(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            if toolbar_primary_button(ui, "开始转换", enabled, PRIMARY_W).clicked() {
                clicked = Some(ToolbarClick::Start);
            }
            let side_group_w = SIDE_W * 2.0 + GAP;
            centered_button_row(ui, side_group_w, GAP, |ui| {
                if toolbar_side_button(ui, "取消", running, SIDE_W).clicked() {
                    clicked = Some(ToolbarClick::Cancel);
                }
                if toolbar_side_button(ui, "打开输出", true, SIDE_W).clicked() {
                    clicked = Some(ToolbarClick::OpenOutput);
                }
            });
        });
    } else {
        let group_w = SIDE_W * 2.0 + PRIMARY_W + GAP * 2.0;
        ui.with_layout(Layout::top_down(egui::Align::Center), |ui| {
            ui.set_width(ui.available_width());
            centered_button_row(ui, group_w, GAP, |ui| {
                if toolbar_side_button(ui, "取消", running, SIDE_W).clicked() {
                    clicked = Some(ToolbarClick::Cancel);
                }
                if toolbar_primary_button(ui, "开始转换", enabled, PRIMARY_W).clicked() {
                    clicked = Some(ToolbarClick::Start);
                }
                if toolbar_side_button(ui, "打开输出", true, SIDE_W).clicked() {
                    clicked = Some(ToolbarClick::OpenOutput);
                }
            });
        });
    }

    clicked
}

/// 在可用宽度内用左右等宽留白，把固定宽度的按钮组居中。
fn centered_button_row(ui: &mut Ui, group_w: f32, gap: f32, add_buttons: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.x = 0.0;
        let pad = ((ui.available_width() - group_w) * 0.5).max(0.0);
        ui.allocate_exact_size(egui::vec2(pad, 1.0), egui::Sense::hover());
        ui.spacing_mut().item_spacing.x = gap;
        add_buttons(ui);
    });
}

fn toolbar_primary_button(ui: &mut Ui, label: &str, enabled: bool, width: f32) -> egui::Response {
    let dark = ui.style().visuals.dark_mode;
    let accent = theme::accent(dark);
    let btn = Button::new(
        RichText::new(label)
            .size(15.0)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(if enabled {
        accent
    } else {
        accent.linear_multiply(0.45)
    })
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(width, 38.0));
    ui.add_enabled(enabled, btn)
}

fn toolbar_side_button(ui: &mut Ui, label: &str, enabled: bool, width: f32) -> egui::Response {
    let dark = ui.style().visuals.dark_mode;
    let btn = Button::new(
        RichText::new(label)
            .size(14.0)
            .color(theme::primary_label(dark)),
    )
    .fill(theme::control_fill(dark))
    .stroke(theme::control_stroke(dark))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(width, 38.0));
    ui.add_enabled(enabled, btn)
}

pub fn folder_field(ui: &mut Ui, label: &str, path: &mut String, enabled: bool) {
    let dark = ui.style().visuals.dark_mode;
    let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;

    if narrow {
        ui.label(
            RichText::new(label)
                .font(theme::section_font())
                .color(theme::primary_label(dark)),
        );
        ui.add_space(4.0);
        if path_field_fill(ui, path, "选择或拖入文件夹…", enabled, true) {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                *path = folder.display().to_string();
            }
        }
    } else {
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(theme::SETTINGS_LABEL_WIDTH, TOOLBAR_ROW_HEIGHT),
                Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        RichText::new(label)
                            .font(theme::section_font())
                            .color(theme::primary_label(dark)),
                    );
                },
            );
            ui.add_space(8.0);
            if path_field_fill(ui, path, "选择或拖入文件夹…", enabled, true) {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    *path = folder.display().to_string();
                }
            }
        });
    }
}

/// 路径输入，可选右侧「浏览…」。整行占满当前 `available_width()`，右缘与全宽控件对齐。
///
/// 返回是否点击了浏览（由调用方打开目录对话框）。
pub fn path_field_fill(
    ui: &mut Ui,
    path: &mut String,
    hint: &str,
    enabled: bool,
    with_browse: bool,
) -> bool {
    const BROWSE_W: f32 = 88.0;
    const GAP: f32 = 8.0;
    let h = TOOLBAR_ROW_HEIGHT;
    let dark = ui.style().visuals.dark_mode;
    let total_w = ui.available_width().max(80.0);
    let (row_rect, _) = ui.allocate_exact_size(egui::vec2(total_w, h), egui::Sense::hover());

    let mut browse_clicked = false;
    if with_browse {
        let browse_rect = egui::Rect::from_min_size(
            egui::pos2(row_rect.max.x - BROWSE_W, row_rect.min.y),
            egui::vec2(BROWSE_W, h),
        );
        let edit_rect = egui::Rect::from_min_max(
            row_rect.min,
            egui::pos2((browse_rect.min.x - GAP).max(row_rect.min.x), row_rect.max.y),
        );

        ui.allocate_ui_at_rect(edit_rect, |ui| {
            ui.set_enabled(enabled);
            ui.add_sized(
                edit_rect.size(),
                TextEdit::singleline(path)
                    .hint_text(hint)
                    .margin(egui::vec2(12.0, 8.0)),
            );
        });
        ui.allocate_ui_at_rect(browse_rect, |ui| {
            ui.set_enabled(enabled);
            browse_clicked = ui
                .add_sized(
                    browse_rect.size(),
                    Button::new(RichText::new("浏览…").size(13.0))
                        .fill(theme::control_fill(dark))
                        .stroke(theme::control_stroke(dark))
                        .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS)),
                )
                .clicked();
        });
    } else {
        ui.allocate_ui_at_rect(row_rect, |ui| {
            ui.set_enabled(enabled);
            ui.add_sized(
                row_rect.size(),
                TextEdit::singleline(path)
                    .hint_text(hint)
                    .margin(egui::vec2(12.0, 8.0)),
            );
        });
    }
    browse_clicked
}

pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    let dark = ui.style().visuals.dark_mode;
    let accent = theme::accent(dark);
    let btn = Button::new(
        RichText::new(label)
            .size(15.0)
            .strong()
            .color(Color32::WHITE),
    )
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
    let btn = Button::new(
        RichText::new(label)
            .size(14.0)
            .color(theme::primary_label(dark)),
    )
    .fill(theme::control_fill(dark))
    .stroke(theme::control_stroke(dark))
    .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
    .min_size(egui::vec2(96.0, 38.0));
    ui.add_enabled(enabled, btn)
}

/// 工具栏用紧凑次要按钮（评审操作栏等，宽度随文案）。
pub fn compact_secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    let dark = ui.style().visuals.dark_mode;
    let btn = Button::new(
        RichText::new(label)
            .size(13.0)
            .color(theme::primary_label(dark)),
    )
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
    let btn = Button::new(
        RichText::new(label)
            .size(13.0)
            .strong()
            .color(Color32::WHITE),
    )
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
pub fn tab_chip_sized(ui: &mut Ui, label: &str, width: f32, selected: bool, enabled: bool) -> bool {
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
    let avail = ui
        .available_width()
        .min(ui.max_rect().width())
        .max(80.0);
    let cell_w = equal_cell_width(avail, gap, COLS);

    ui.set_max_width(avail);
    ui.set_width(avail);
    for (row_idx, chunk) in tabs.chunks(COLS).enumerate() {
        ui.horizontal(|ui| {
            ui.set_max_width(avail);
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
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(1.0, Color32::from_white_alpha(180)),
    );
}

/// 顶部模式切换条：轨道拉满父级宽度，与下方内容列左右对齐；分段等分。
pub fn mode_tab_bar<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) {
    if options.len() < 2 {
        return;
    }

    let dark = ui.style().visuals.dark_mode;
    let accent = theme::accent(dark);
    let seg_h = 34.0;
    let n = options.len() as f32;
    let track_w = ui.available_width();
    ui.set_width(track_w);

    Frame::new()
        .fill(theme::segment_track_fill(dark))
        .stroke(theme::separator_stroke(dark))
        .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let gap = 3.0;
            let inner_w = ui.available_width();
            let seg_w = ((inner_w - gap * (n - 1.0)) / n).max(64.0);

            ui.horizontal(|ui| {
                ui.set_width(inner_w);
                ui.spacing_mut().item_spacing.x = gap;
                for (option, label) in options {
                    let selected = *value == *option;
                    let (fill, stroke, fg) = if selected {
                        (accent, Stroke::NONE, Color32::WHITE)
                    } else {
                        (
                            Color32::TRANSPARENT,
                            Stroke::NONE,
                            theme::secondary_label(dark),
                        )
                    };

                    let text = RichText::new(*label).size(13.5).color(fg);
                    let text = if selected { text.strong() } else { text };
                    let btn = Button::new(text)
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(CornerRadius::same(
                            theme::CONTROL_RADIUS.saturating_sub(2),
                        ))
                        .min_size(egui::vec2(seg_w, seg_h));

                    if ui.add_sized(egui::vec2(seg_w, seg_h), btn).clicked() {
                        *value = *option;
                    }
                }
            });
        });
}

pub fn error_banner(ui: &mut Ui, text: &str) {
    semantic_banner(ui, text, theme::error_color(ui.style().visuals.dark_mode));
}

pub fn warning_banner(ui: &mut Ui, text: &str) {
    semantic_banner(ui, text, theme::warning_color(ui.style().visuals.dark_mode));
}

fn semantic_banner(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.linear_multiply(0.12))
        .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(14, 10))
        .stroke(Stroke::new(1.0, color.linear_multiply(0.55)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(13.5).color(color));
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
            ui.spacing_mut().item_spacing.x = 0.0;
            settings_label(ui, label, dark);
            ui.add_space(8.0);
            let w = ui.available_width().max(40.0);
            ui.allocate_ui_with_layout(
                egui::vec2(w, ui.available_height().max(1.0)),
                Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(w);
                    add_contents(ui)
                },
            )
            .inner
        })
        .inner
    }
}

/// 与 [`settings_labeled_row`] 标签列对齐的缩进区域（用于操作按钮等）。
pub fn settings_indented<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let narrow = ui.available_width() < theme::NARROW_BREAKPOINT;
    if narrow {
        add_contents(ui)
    } else {
        // 与 settings_labeled_row 完全同一套间距，避免多出/少出 item_spacing 导致左右不齐
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(theme::SETTINGS_LABEL_WIDTH, ui.spacing().interact_size.y),
                Layout::left_to_right(egui::Align::Center),
                |_| {},
            );
            ui.add_space(8.0);
            let w = ui.available_width().max(40.0);
            ui.allocate_ui_with_layout(
                egui::vec2(w, ui.spacing().interact_size.y.max(TOOLBAR_ROW_HEIGHT)),
                Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(w);
                    add_contents(ui)
                },
            )
            .inner
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
    let (fill, stroke, fg) = if running {
        (
            theme::accent(dark).linear_multiply(0.14),
            Stroke::new(1.0, theme::accent(dark).linear_multiply(0.45)),
            theme::accent(dark),
        )
    } else {
        (
            theme::log_fill(dark),
            theme::separator_stroke(dark),
            theme::secondary_label(dark),
        )
    };

    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(14, 10))
        .stroke(stroke)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if running {
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(dot.center(), 3.5, theme::accent(dark));
                }
                ui.label(RichText::new(text).size(13.5).color(fg));
            });
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
    ui.add_space(2.0);
    ui.label(
        RichText::new("可将文件夹拖入窗口，或点「浏览…」选择路径")
            .size(12.0)
            .color(theme::secondary_label(dark)),
    );
}
