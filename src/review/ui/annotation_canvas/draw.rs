//! 标注绘制辅助（箭头头部、半透明矩形、选中控制点）。

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, Vec2};

use crate::review::domain::coords::egui_bridge;

use super::coords::{norm_rect_to_screen, norm_to_screen_pos2, NormPoint, NormRect, ViewportTransform};
use super::types::{Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle};

const HANDLE_RADIUS: f32 = 5.0;

pub fn stroke_from_style(style: &AnnotationStyle) -> Stroke {
  Stroke::new(
    style.line_width,
    Color32::from_rgba_unmultiplied(
      style.color[0],
      style.color[1],
      style.color[2],
      style.color[3],
    ),
  )
}

pub fn color_from_style(style: &AnnotationStyle) -> Color32 {
  Color32::from_rgba_unmultiplied(
    style.color[0],
    style.color[1],
    style.color[2],
    style.color[3],
  )
}

pub fn fill_from_style(style: &AnnotationStyle) -> Color32 {
  Color32::from_rgba_unmultiplied(
    style.color[0],
    style.color[1],
    style.color[2],
    (style.color[3] as f32 * 0.25) as u8,
  )
}

/// 绘制单条标注。
pub fn paint_annotation(
  painter: &Painter,
  ann: &Annotation,
  transform: &ViewportTransform,
  selected: bool,
) {
  let stroke = stroke_from_style(&ann.style);
  let color = color_from_style(&ann.style);
  match (&ann.kind, &ann.position) {
    (AnnotationKind::Rectangle, AnnotationPosition::Rectangle(r)) => {
      let rect = norm_rect_to_screen(
        NormRect {
          x0: r.x0,
          y0: r.y0,
          x1: r.x1,
          y1: r.y1,
        },
        transform,
      );
      let egui_rect = egui_bridge::to_egui_rect(rect);
      painter.rect_filled(egui_rect, 0.0, fill_from_style(&ann.style));
      painter.rect_stroke(egui_rect, 0.0, stroke, egui::StrokeKind::Outside);
      if selected {
        paint_rect_handles(painter, egui_rect, color);
      }
    }
    (AnnotationKind::Arrow, AnnotationPosition::Arrow(a)) => {
      let p0 = egui_bridge::to_egui_pos(norm_to_screen_pos2(
        NormPoint { x: a.x0, y: a.y0 },
        transform,
      ));
      let p1 = egui_bridge::to_egui_pos(norm_to_screen_pos2(
        NormPoint { x: a.x1, y: a.y1 },
        transform,
      ));
      painter.line_segment([p0, p1], stroke);
      paint_arrow_head(painter, p0, p1, stroke);
      if selected {
        painter.circle_filled(p0, HANDLE_RADIUS, color);
        painter.circle_filled(p1, HANDLE_RADIUS, color);
      }
    }
    (AnnotationKind::Text, AnnotationPosition::Text(t)) => {
      let p = egui_bridge::to_egui_pos(norm_to_screen_pos2(
        NormPoint { x: t.x, y: t.y },
        transform,
      ));
      let text = if ann.content.is_empty() {
        "文字"
      } else {
        &ann.content
      };
      let font = egui::FontId::proportional(14.0);
      let galley = painter.layout_no_wrap(text.to_string(), font.clone(), color);
      let bg = Rect::from_min_size(p, galley.size() + Vec2::new(8.0, 4.0));
      painter.rect_filled(bg, 3.0, Color32::from_black_alpha(160));
      painter.text(p + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, text, font, color);
      if selected {
        painter.rect_stroke(bg, 3.0, stroke, egui::StrokeKind::Outside);
        painter.circle_filled(p, HANDLE_RADIUS, color);
      }
    }
    _ => {}
  }
}

pub fn paint_arrow_head(painter: &Painter, from: Pos2, to: Pos2, stroke: Stroke) {
  let dir = to - from;
  let len = dir.length();
  if len < 1.0 {
    return;
  }
  let ux = dir.x / len;
  let uy = dir.y / len;
  let size = 10.0;
  let left = Pos2::new(to.x - ux * size - uy * size * 0.5, to.y - uy * size + ux * size * 0.5);
  let right = Pos2::new(to.x - ux * size + uy * size * 0.5, to.y - uy * size - ux * size * 0.5);
  painter.line_segment([to, left], stroke);
  painter.line_segment([to, right], stroke);
}

fn paint_rect_handles(painter: &Painter, rect: Rect, color: Color32) {
  for corner in rect_corners(rect) {
    painter.circle_filled(corner, HANDLE_RADIUS, color);
  }
}

fn rect_corners(rect: Rect) -> [Pos2; 4] {
  [
    rect.left_top(),
    rect.right_top(),
    rect.right_bottom(),
    rect.left_bottom(),
  ]
}

/// 命中测试：返回最上层标注 id。
pub fn hit_test(annotations: &[Annotation], transform: &ViewportTransform, point: Pos2) -> Option<i64> {
  annotations
    .iter()
    .rev()
    .find(|ann| hit_annotation(ann, transform, point))
    .map(|a| a.id)
}

fn hit_annotation(ann: &Annotation, transform: &ViewportTransform, point: Pos2) -> bool {
  const TOL: f32 = 6.0;
  match (&ann.kind, &ann.position) {
    (AnnotationKind::Rectangle, AnnotationPosition::Rectangle(r)) => {
      let rect = egui_bridge::to_egui_rect(norm_rect_to_screen(
        NormRect {
          x0: r.x0,
          y0: r.y0,
          x1: r.x1,
          y1: r.y1,
        },
        transform,
      ));
      rect.expand(TOL).contains(point)
    }
    (AnnotationKind::Arrow, AnnotationPosition::Arrow(a)) => {
      let p0 = egui_bridge::to_egui_pos(norm_to_screen_pos2(
        NormPoint { x: a.x0, y: a.y0 },
        transform,
      ));
      let p1 = egui_bridge::to_egui_pos(norm_to_screen_pos2(
        NormPoint { x: a.x1, y: a.y1 },
        transform,
      ));
      dist_to_segment(point, p0, p1) <= TOL
        || point.distance(p0) <= TOL + HANDLE_RADIUS
        || point.distance(p1) <= TOL + HANDLE_RADIUS
    }
    (AnnotationKind::Text, AnnotationPosition::Text(t)) => {
      let p = egui_bridge::to_egui_pos(norm_to_screen_pos2(
        NormPoint { x: t.x, y: t.y },
        transform,
      ));
      let text = if ann.content.is_empty() {
        "文字"
      } else {
        &ann.content
      };
      let w = (text.len() as f32 * 7.0).clamp(24.0, 160.0);
      Rect::from_min_size(p, Vec2::new(w + 8.0, 20.0)).contains(point)
    }
    _ => false,
  }
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
  let ab = b - a;
  let len_sq = ab.length_sq();
  if len_sq < 1e-6 {
    return p.distance(a);
  }
  let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
  let proj = a + ab * t;
  p.distance(proj)
}
