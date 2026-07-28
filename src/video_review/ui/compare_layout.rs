//! 多路对比布局：预设格子 + 按画幅比例装箱。

use eframe::egui::{pos2, Rect, Vec2};

pub use crate::gui::prefs::VideoCompareLayoutPref as CompareLayoutPreset;

impl CompareLayoutPreset {
    pub const ALL: [CompareLayoutPreset; 6] = [
        Self::Auto,
        Self::TwoH,
        Self::TwoV,
        Self::Grid2x2,
        Self::Grid3x2,
        Self::OnePlusFive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "自动",
            Self::TwoH => "2H",
            Self::TwoV => "2V",
            Self::Grid2x2 => "2×2",
            Self::Grid3x2 => "3×2",
            Self::OnePlusFive => "1+5",
        }
    }

    /// 路数不足时禁用。
    pub fn enabled_for(self, n: usize) -> bool {
        match self {
            Self::Auto => n >= 1,
            Self::TwoH | Self::TwoV => n >= 2,
            Self::Grid2x2 => n >= 3,
            Self::Grid3x2 => n >= 5,
            Self::OnePlusFive => n >= 2,
        }
    }

    pub fn resolve(self, n: usize) -> CompareLayoutPreset {
        match self {
            Self::Auto => match n {
                0 | 1 => Self::Auto,
                2 => Self::TwoH,
                3 | 4 => Self::Grid2x2,
                _ => Self::Grid3x2,
            },
            other => other,
        }
    }
}

/// 对比区显示模式（与布局预设正交）。
#[derive(Debug, Clone, PartialEq)]
pub enum CompareViewMode {
    Grid,
    Solo {
        video_id: i64,
    },
    Wipe {
        left: i64,
        right: i64,
        split: f32,
    },
    Overlay {
        base: i64,
        top: i64,
        opacity: f32,
        diff: bool,
    },
}

impl Default for CompareViewMode {
    fn default() -> Self {
        Self::Grid
    }
}

impl CompareViewMode {
    pub fn is_grid(&self) -> bool {
        matches!(self, Self::Grid)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PaneSlot {
    pub video_id: i64,
    /// 整格（含标题栏区域）。
    pub rect: Rect,
    /// 画面区（扣除标题栏）。
    pub content: Rect,
}

const GAP: f32 = 6.0;
const DEFAULT_ASPECT: f32 = 16.0 / 9.0;

pub fn video_aspect(width: u32, height: u32) -> f32 {
    if width > 0 && height > 0 {
        width as f32 / height as f32
    } else {
        DEFAULT_ASPECT
    }
}

/// 在 `area` 内按预设与画幅比装箱。
pub fn layout_slots(
    area: Rect,
    preset: CompareLayoutPreset,
    ids: &[i64],
    aspects: &[f32],
    title_h: f32,
) -> Vec<PaneSlot> {
    if ids.is_empty() || area.width() < 8.0 || area.height() < 8.0 {
        return Vec::new();
    }
    debug_assert_eq!(ids.len(), aspects.len());

    let n = ids.len();
    let aspects = &aspects[..n.min(aspects.len())];
    let resolved = preset.resolve(n);
    let cells = match (resolved, n) {
        (_, 1) => vec![area],
        (CompareLayoutPreset::TwoV, _) => pack_col(area, aspects),
        (CompareLayoutPreset::TwoH, _) => pack_row(area, aspects),
        (CompareLayoutPreset::OnePlusFive, _) => pack_one_plus_five(area, n, aspects),
        (CompareLayoutPreset::Grid2x2, _) => pack_grid(area, n, 2, aspects),
        (CompareLayoutPreset::Grid3x2, _) => pack_grid(area, n, 3, aspects),
        (CompareLayoutPreset::Auto, _) => pack_grid(area, n, if n <= 4 { 2 } else { 3 }, aspects),
    };

    cells
        .into_iter()
        .zip(ids.iter().copied())
        .map(|(rect, video_id)| {
            let content = content_rect(rect, title_h);
            PaneSlot {
                video_id,
                rect,
                content,
            }
        })
        .collect()
}

fn content_rect(rect: Rect, title_h: f32) -> Rect {
    let th = title_h.clamp(0.0, (rect.height() - 40.0).max(0.0));
    Rect::from_min_max(
        pos2(rect.min.x, rect.min.y + th),
        pos2(rect.max.x, rect.max.y),
    )
}

/// 同行按宽高比分配宽度，尽量让各格 contain 时无黑边。
fn pack_row(area: Rect, aspects: &[f32]) -> Vec<Rect> {
    if aspects.is_empty() {
        return Vec::new();
    }
    let gaps = GAP * (aspects.len().saturating_sub(1) as f32);
    let avail_w = (area.width() - gaps).max(1.0);
    let avail_h = area.height().max(1.0);

    let aspects: Vec<f32> = aspects
        .iter()
        .map(|a| if *a > 0.05 { *a } else { DEFAULT_ASPECT })
        .collect();

    // 理想：同高，宽 = h * aspect
    let sum_ar: f32 = aspects.iter().sum();
    let ideal_h = (avail_w / sum_ar).min(avail_h);
    let scale = if ideal_h * sum_ar > avail_w {
        avail_w / (ideal_h * sum_ar).max(1e-3)
    } else {
        1.0
    };
    let h = ideal_h * scale;
    let y = area.min.y + (avail_h - h) * 0.5;

    let mut x = area.min.x;
    let mut total_w: f32 = aspects.iter().map(|a| h * a).sum();
    total_w += gaps;
    if total_w < area.width() {
        x += (area.width() - total_w) * 0.5;
    }

    let mut out = Vec::with_capacity(aspects.len());
    for (i, ar) in aspects.iter().enumerate() {
        let w = (h * ar).max(40.0);
        out.push(Rect::from_min_size(pos2(x, y), Vec2::new(w, h)));
        x += w;
        if i + 1 < aspects.len() {
            x += GAP;
        }
    }
    out
}

fn pack_col(area: Rect, aspects: &[f32]) -> Vec<Rect> {
    if aspects.is_empty() {
        return Vec::new();
    }
    let gaps = GAP * (aspects.len().saturating_sub(1) as f32);
    let avail_w = area.width().max(1.0);
    let avail_h = (area.height() - gaps).max(1.0);

    let aspects: Vec<f32> = aspects
        .iter()
        .map(|a| if *a > 0.05 { *a } else { DEFAULT_ASPECT })
        .collect();

    // 同宽，高 = w / aspect
    let sum_inv: f32 = aspects.iter().map(|a| 1.0 / a).sum();
    let ideal_w = (avail_h / sum_inv).min(avail_w);
    let scale = if ideal_w * sum_inv > avail_h {
        avail_h / (ideal_w * sum_inv).max(1e-3)
    } else {
        1.0
    };
    let w = ideal_w * scale;
    let x = area.min.x + (avail_w - w) * 0.5;

    let mut y = area.min.y;
    let mut total_h: f32 = aspects.iter().map(|a| w / a).sum();
    total_h += gaps;
    if total_h < area.height() {
        y += (area.height() - total_h) * 0.5;
    }

    let mut out = Vec::with_capacity(aspects.len());
    for (i, ar) in aspects.iter().enumerate() {
        let h = (w / ar).max(40.0);
        out.push(Rect::from_min_size(pos2(x, y), Vec2::new(w, h)));
        y += h;
        if i + 1 < aspects.len() {
            y += GAP;
        }
    }
    out
}

fn pack_grid(area: Rect, n: usize, cols: usize, aspects: &[f32]) -> Vec<Rect> {
    let cols = cols.max(1);
    let rows = (n + cols - 1) / cols;
    let row_h = (area.height() - GAP * (rows.saturating_sub(1) as f32)) / rows as f32;
    let mut out = Vec::with_capacity(n);
    for r in 0..rows {
        let start = r * cols;
        let end = (start + cols).min(n);
        if start >= end {
            break;
        }
        let y0 = area.min.y + r as f32 * (row_h + GAP);
        let row_rect = Rect::from_min_size(pos2(area.min.x, y0), Vec2::new(area.width(), row_h));
        let row_aspects = &aspects[start..end];
        out.extend(pack_row(row_rect, row_aspects));
    }
    out
}

/// 左侧主画面（约 2/3），右侧最多 5 路竖排。
fn pack_one_plus_five(area: Rect, n: usize, aspects: &[f32]) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![area];
    }
    let main_w = area.width() * 0.62;
    let side_w = area.width() - main_w - GAP;
    let main_rect = Rect::from_min_size(area.min, Vec2::new(main_w.max(80.0), area.height()));
    let side_rect = Rect::from_min_size(
        pos2(area.min.x + main_w + GAP, area.min.y),
        Vec2::new(side_w.max(60.0), area.height()),
    );

    let mut out = pack_row(main_rect, &aspects[..1]);
    let side_n = (n - 1).min(5);
    out.extend(pack_col(side_rect, &aspects[1..1 + side_n]));
    // 若超过 6 路，多余的挤进侧栏继续
    if n > 6 {
        // 已在 side 放了 5；剩余忽略（对比上限 6）
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_two_uses_two_h() {
        assert_eq!(
            CompareLayoutPreset::Auto.resolve(2),
            CompareLayoutPreset::TwoH
        );
    }

    #[test]
    fn pack_row_portrait_landscape_widths_differ() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(800.0, 400.0));
        let cells = pack_row(area, &[16.0 / 9.0, 9.0 / 16.0]);
        assert_eq!(cells.len(), 2);
        assert!(cells[0].width() > cells[1].width());
    }

    #[test]
    fn one_plus_five_six_slots() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(900.0, 500.0));
        let ids: Vec<i64> = (1..=6).collect();
        let aspects = vec![16.0 / 9.0; 6];
        let slots = layout_slots(area, CompareLayoutPreset::OnePlusFive, &ids, &aspects, 28.0);
        assert_eq!(slots.len(), 6);
        assert!(slots[0].rect.width() > slots[1].rect.width());
    }
}
