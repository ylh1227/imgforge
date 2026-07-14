//! 评审面板辅助 UI 与格式化函数。

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::review::ui::review_panel_types::BatchOpKind;

pub(crate) fn viewport_size(ctx: &egui::Context) -> egui::Vec2 {
    ctx.input(|i| {
        i.viewport()
            .inner_rect
            .map(|r| r.size())
            .unwrap_or_else(|| ctx.screen_rect().size())
    })
}

pub(crate) fn file_mtime_key(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().saturating_mul(1_000_000_000) + d.subsec_nanos() as u64)
}

pub(crate) fn histogram_ui(ui: &mut egui::Ui, bins: &[u32], color: egui::Color32, height: f32) {
    let width = ui.available_width().max(128.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_black_alpha(24));
    let max = bins.iter().copied().max().unwrap_or(1).max(1) as f32;
    let bar_w = rect.width() / bins.len().max(1) as f32;
    for (idx, count) in bins.iter().enumerate() {
        let x0 = rect.left() + idx as f32 * bar_w;
        let x1 = (x0 + bar_w).min(rect.right());
        let h = (*count as f32 / max) * rect.height();
        let bar = egui::Rect::from_min_max(
            egui::pos2(x0, rect.bottom() - h),
            egui::pos2(x1.max(x0 + 1.0), rect.bottom()),
        );
        painter.rect_filled(bar, 0.0, color.linear_multiply(0.85));
    }
}

pub(crate) fn format_contact_sheets(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        "无".into()
    } else if paths.len() == 1 {
        paths[0].display().to_string()
    } else {
        format!("{} 页", paths.len())
    }
}

pub(crate) fn batch_op_description(op: BatchOpKind) -> &'static str {
    match op {
        BatchOpKind::SetStatus(_) => "将对所选图片批量更新评审状态，是否继续？",
        BatchOpKind::ClearAnnotations => "将清空所选图片的全部标注，是否继续？",
        BatchOpKind::AddRemark => "将对所选图片批量写入备注，是否继续？",
        BatchOpKind::CopyCurrentAnnotations => "将把当前图片的首条标注复制到所选图片，是否继续？",
    }
}

pub(crate) fn annotation_kind_label(kind: crate::review::domain::AnnotationKind) -> &'static str {
    use crate::review::domain::AnnotationKind;
    match kind {
        AnnotationKind::Rectangle => "矩形",
        AnnotationKind::Arrow => "箭头",
        AnnotationKind::Text => "文字",
    }
}

pub(crate) fn truncate_text(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text.to_string()
    } else {
        format!("{}…", chars[..max_chars].iter().collect::<String>())
    }
}
