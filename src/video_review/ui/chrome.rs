//! 视频评审视觉：审片暗场、时间码、芯片组。

use eframe::egui::{self, CornerRadius, Frame, Margin, RichText, Ui};

use crate::gui::{theme, widgets};

/// 页眉：短副标题 + 当前片段/同步时间码。
pub fn review_page_header(ui: &mut Ui, clip_line: Option<&str>, sync_ms: Option<u64>) {
    let dark = ui.visuals().dark_mode;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            RichText::new("片段 · 对比 · 抽帧")
                .size(13.0)
                .color(theme::secondary_label(dark)),
        );
        if let Some(clip) = clip_line.filter(|s| !s.is_empty()) {
            ui.label(RichText::new("·").weak().size(12.0));
            let clipped = if clip.chars().count() > 36 {
                let s: String = clip.chars().take(34).collect();
                format!("{s}…")
            } else {
                clip.to_string()
            };
            ui.label(
                RichText::new(clipped)
                    .size(12.0)
                    .color(theme::primary_label(dark)),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(ms) = sync_ms {
                timecode_label(ui, ms, false);
                ui.add_space(4.0);
                ui.label(
                    RichText::new("SYNC")
                        .size(10.0)
                        .color(theme::secondary_label(dark)),
                );
            }
        });
    });
    widgets::page_header_gap(ui);
}

/// 等宽感时间码（实际用等宽数字字号）。
pub fn timecode_label(ui: &mut Ui, ms: u64, on_stage: bool) {
    let dark = ui.visuals().dark_mode;
    let color = if on_stage {
        theme::video_timecode_on_stage()
    } else {
        theme::video_timecode_color(dark)
    };
    ui.label(
        RichText::new(crate::video_review::ui::multi_compare::format_ms(ms))
            .monospace()
            .size(13.0)
            .color(color),
    );
}

/// 弱标签 + 芯片横排（对齐质量/模式、倍速等）。
///
/// 直接写入父级横向布局（不再套一层 `horizontal`），避免嵌套行高把同行顶沉。
pub fn chip_strip(ui: &mut Ui, label: &str, add_chips: impl FnOnce(&mut Ui)) {
    let dark = ui.visuals().dark_mode;
    let h = widgets::TOOLBAR_ROW_HEIGHT;
    if !label.is_empty() {
        let label_w = ui.fonts(|f| {
            f.layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(11.0),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
        }) + 2.0;
        ui.add_sized(
            egui::vec2(label_w, h),
            egui::Label::new(
                RichText::new(label)
                    .size(11.0)
                    .color(theme::secondary_label(dark)),
            )
            .selectable(false),
        );
    }
    Frame::new()
        .fill(theme::segment_track_fill(dark))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(4, 0))
        .show(ui, |ui| {
            ui.set_min_height(h);
            ui.set_height(h);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                add_chips(ui);
            });
        });
}

/// 工具栏内可选标签（固定行高，避免与按钮沉底错位）。
pub fn toolbar_toggle(ui: &mut Ui, selected: bool, text: &str) -> egui::Response {
    let h = widgets::TOOLBAR_ROW_HEIGHT;
    let w = ui.fonts(|f| {
        f.layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(13.0),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
    }) + 16.0;
    ui.add_sized(
        egui::vec2(w.max(40.0), h),
        egui::SelectableLabel::new(selected, RichText::new(text).size(13.0)),
    )
}

/// 监视器井：包住对比/单路画面。
pub fn stage_frame<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let dark = ui.visuals().dark_mode;
    Frame::new()
        .fill(theme::video_stage_fill(dark))
        .stroke(theme::video_stage_stroke(dark))
        .corner_radius(CornerRadius::same(theme::VIDEO_STAGE_RADIUS))
        .inner_margin(Margin::same(6))
        .show(ui, add_contents)
        .inner
}

/// 工具条行之间的细分隔（比 inset_separator 更紧）。
pub fn toolbar_row_gap(ui: &mut Ui) {
    ui.add_space(theme::VIDEO_TOOLBAR_ROW_GAP);
}
