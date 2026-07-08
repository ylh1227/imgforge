//! 标注离屏渲染：导出烧录与归一化坐标叠加绘制。

use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_hollow_rect_mut, draw_line_segment_mut};
use imageproc::rect::Rect;

use super::annotation::{Annotation, AnnotationKind, AnnotationPosition, RectanglePosition};
use super::coords::{norm_to_pixel, NormPoint};

/// 将标注烧录到输出图（用于导出叠加）。
pub fn burn_annotations_onto(base: &mut RgbaImage, annotations: &[Annotation]) {
    let size = (base.width(), base.height());
    for ann in annotations {
        let color = Rgba(ann.style.color);
        let lw = ann.style.line_width.max(1.0) as i32;
        match (&ann.kind, &ann.position) {
            (AnnotationKind::Rectangle, AnnotationPosition::Rectangle(r)) => {
                let rect = norm_rect_to_pixel_rect(r, size);
                if lw <= 1 {
                    draw_hollow_rect_mut(base, rect, color);
                } else {
                    for i in 0..lw {
                        let inset = Rect::at(rect.left() + i, rect.top() + i).of_size(
                            rect.width().saturating_sub((2 * i) as u32),
                            rect.height().saturating_sub((2 * i) as u32),
                        );
                        draw_hollow_rect_mut(base, inset, color);
                    }
                }
            }
            (AnnotationKind::Arrow, AnnotationPosition::Arrow(a)) => {
                let p0 = norm_to_pixel(NormPoint { x: a.x0, y: a.y0 }, size);
                let p1 = norm_to_pixel(NormPoint { x: a.x1, y: a.y1 }, size);
                draw_line_segment_mut(
                    base,
                    (p0.x as f32, p0.y as f32),
                    (p1.x as f32, p1.y as f32),
                    color,
                );
                draw_arrow_head(base, p0, p1, color);
            }
            (AnnotationKind::Text, AnnotationPosition::Text(t)) => {
                let p = norm_to_pixel(NormPoint { x: t.x, y: t.y }, size);
                let text = if ann.content.is_empty() {
                    "备注"
                } else {
                    &ann.content
                };
                // 轻量实现：用色块标记文字锚点 + 首字符示意（完整字体渲染留给 UI 层）
                let marker =
                    Rect::at(p.x, p.y).of_size((text.len() as u32 * 6).min(120).max(12), 14);
                draw_filled_rect_mut(base, marker, color);
            }
            _ => {}
        }
    }
}

/// 生成标注叠加缓存键（图片路径 + 标注 id 列表）。
pub fn render_cache_key(path: &str, annotations: &[Annotation]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    for a in annotations {
        a.id.hash(&mut h);
        (a.kind.db_value() as u64).hash(&mut h);
    }
    h.finish()
}

/// 占位：UI 层主要用 egui 绘制；离屏缓存供纹理复用。
pub fn render_annotations_overlay(
    size: (u32, u32),
    annotations: &[Annotation],
) -> Option<image::RgbaImage> {
    if annotations.is_empty() || size.0 == 0 || size.1 == 0 {
        return None;
    }
    let mut overlay = image::RgbaImage::new(size.0, size.1);
    burn_annotations_onto(&mut overlay, annotations);
    Some(overlay)
}

fn norm_rect_to_pixel_rect(r: &RectanglePosition, size: (u32, u32)) -> Rect {
    let p0 = norm_to_pixel(NormPoint { x: r.x0, y: r.y0 }, size);
    let p1 = norm_to_pixel(NormPoint { x: r.x1, y: r.y1 }, size);
    let x = p0.x.min(p1.x);
    let y = p0.y.min(p1.y);
    let w = (p1.x - p0.x).unsigned_abs();
    let h = (p1.y - p0.y).unsigned_abs();
    Rect::at(x, y).of_size(w.max(1) as u32, h.max(1) as u32)
}

fn draw_arrow_head(
    img: &mut RgbaImage,
    from: super::coords::PixelPoint,
    to: super::coords::PixelPoint,
    color: Rgba<u8>,
) {
    let dx = (to.x - from.x) as f32;
    let dy = (to.y - from.y) as f32;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let size = 8.0;
    let left = (
        to.x as f32 - ux * size - uy * size * 0.5,
        to.y as f32 - uy * size + ux * size * 0.5,
    );
    let right = (
        to.x as f32 - ux * size + uy * size * 0.5,
        to.y as f32 - uy * size - ux * size * 0.5,
    );
    draw_line_segment_mut(img, (to.x as f32, to.y as f32), left, color);
    draw_line_segment_mut(img, (to.x as f32, to.y as f32), right, color);
}
