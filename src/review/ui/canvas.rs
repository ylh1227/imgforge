//! 画布：缩放、平移、标注绘制与纹理缓存。

use std::collections::HashMap;
use std::path::Path;

use eframe::egui::{self, Color32, Context, Painter, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, Vec2};
use image::GenericImageView;

use crate::review::domain::annotation::{
  Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle, ArrowPosition, RectanglePosition,
  TextPosition,
};
use crate::review::domain::coords::{
  egui_bridge, norm_rect_to_screen, norm_to_screen_pos2, screen_rect_to_norm, screen_to_norm,
  NormPoint, NormRect, ScreenPoint, ViewportTransform,
};
use crate::review::domain::render_cache_key;

/// 当前绘制工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawTool {
  #[default]
  Pan,
  Rectangle,
  Arrow,
  Text,
}

/// 画布交互状态。
pub struct CanvasState {
  pub tool: DrawTool,
  pub style: AnnotationStyle,
  pub transform: ViewportTransform,
  pub image_size: (u32, u32),
  pub annotations: Vec<Annotation>,
  pub draft_start: Option<Pos2>,
  pub text_input: String,
  pub use_full_res: bool,
  texture_cache: HashMap<String, TextureHandle>,
  annotation_cache_key: u64,
}

impl CanvasState {
  pub fn new() -> Self {
    Self {
      tool: DrawTool::Pan,
      style: AnnotationStyle::default(),
      transform: ViewportTransform::fit_image(egui_bridge::vec2(Vec2::new(400.0, 300.0)), (1, 1)),
      image_size: (1, 1),
      annotations: Vec::new(),
      draft_start: None,
      text_input: String::new(),
      use_full_res: false,
      texture_cache: HashMap::new(),
      annotation_cache_key: 0,
    }
  }

  pub fn fit_window(&mut self, canvas: Vec2) {
    self.transform = ViewportTransform::fit_image(egui_bridge::vec2(canvas), self.image_size);
    self.use_full_res = false;
  }

  pub fn actual_size(&mut self, canvas: Vec2) {
    self.transform =
      ViewportTransform::one_to_one(egui_bridge::vec2(canvas), self.image_size);
    self.use_full_res = true;
  }

  pub fn set_annotations(&mut self, path: &str, annotations: Vec<Annotation>) {
    self.annotation_cache_key = render_cache_key(path, &annotations);
    self.annotations = annotations;
  }

  pub fn load_texture(
    &mut self,
    ctx: &Context,
    path: &Path,
    thumb: Option<&Path>,
  ) -> Option<TextureHandle> {
    let key = path.to_string_lossy().to_string();
    if let Some(tex) = self.texture_cache.get(&key) {
      return Some(tex.clone());
    }
    let load_path = if self.use_full_res {
      path
    } else {
      thumb.filter(|p| p.exists()).unwrap_or(path)
    };
    let img = image::open(load_path).ok()?;
    self.image_size = img.dimensions();
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let tex = ctx.load_texture(format!("review_{key}"), color_image, egui::TextureOptions::LINEAR);
    self.texture_cache.insert(key, tex.clone());
    Some(tex)
  }

  /// 绘制主画布，返回新完成的标注（若有）。
  pub fn show(
    &mut self,
    ui: &mut Ui,
    ctx: &Context,
    image_path: &Path,
    thumb: Option<&Path>,
  ) -> Option<Annotation> {
    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    if response.hovered() {
      let scroll = ui.input(|i| i.raw_scroll_delta.y);
      if scroll != 0.0 {
        let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
        self.transform.zoom = (self.transform.zoom * factor).clamp(0.1, 16.0);
      }
    }
    if response.dragged() && self.tool == DrawTool::Pan {
      let delta = response.drag_delta();
      self.transform.pan.x += delta.x;
      self.transform.pan.y += delta.y;
    }

    if self.image_size == (1, 1) || self.transform.image_rect.size.x < 2.0 {
      self.fit_window(rect.size());
    }

    if let Some(tex) = self.load_texture(ctx, image_path, thumb) {
      let img_rect = egui_bridge::to_egui_rect(self.transform.displayed_image_rect());
      painter.image(tex.id(), img_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }

    self.draw_annotations(&painter);

    if let Some(start) = self.draft_start {
      if let Some(pointer) = ui.input(|i| i.pointer.latest_pos()) {
        if pointer.is_finite() {
          let stroke = stroke_from_style(&self.style);
          match self.tool {
            DrawTool::Rectangle => {
              painter.rect_stroke(
                Rect::from_two_pos(start, pointer),
                0.0,
                stroke,
                egui::StrokeKind::Outside,
              );
            }
            DrawTool::Arrow => {
              painter.line_segment([start, pointer], stroke);
            }
            _ => {}
          }
        }
      }
    }

    if response.clicked() {
      if let Some(pos) = response.interact_pointer_pos() {
        match self.tool {
          DrawTool::Text => {
            let norm = screen_to_norm(ScreenPoint { x: pos.x, y: pos.y }, &self.transform);
            if !self.text_input.trim().is_empty() {
              return Some(Annotation::new_draft(
                0,
                AnnotationKind::Text,
                AnnotationPosition::Text(TextPosition { x: norm.x, y: norm.y }),
                self.style.clone(),
                self.text_input.clone(),
              ));
            }
          }
          DrawTool::Rectangle | DrawTool::Arrow => {
            if self.draft_start.is_none() {
              self.draft_start = Some(pos);
            } else if let Some(start) = self.draft_start.take() {
              let a = ScreenPoint { x: start.x, y: start.y };
              let b = ScreenPoint { x: pos.x, y: pos.y };
              let norm = screen_rect_to_norm(a, b, &self.transform);
              return match self.tool {
                DrawTool::Rectangle => Some(Annotation::new_draft(
                  0,
                  AnnotationKind::Rectangle,
                  AnnotationPosition::Rectangle(RectanglePosition {
                    x0: norm.x0,
                    y0: norm.y0,
                    x1: norm.x1,
                    y1: norm.y1,
                  }),
                  self.style.clone(),
                  String::new(),
                )),
                DrawTool::Arrow => Some(Annotation::new_draft(
                  0,
                  AnnotationKind::Arrow,
                  AnnotationPosition::Arrow(ArrowPosition {
                    x0: norm.x0,
                    y0: norm.y0,
                    x1: norm.x1,
                    y1: norm.y1,
                  }),
                  self.style.clone(),
                  String::new(),
                )),
                _ => None,
              };
            }
          }
          DrawTool::Pan => {}
        }
      }
    }

    None
  }

  fn draw_annotations(&self, painter: &Painter) {
    for ann in &self.annotations {
      let stroke = stroke_from_style(&ann.style);
      let color = Color32::from_rgba_unmultiplied(
        ann.style.color[0],
        ann.style.color[1],
        ann.style.color[2],
        ann.style.color[3],
      );
      match (&ann.kind, &ann.position) {
        (AnnotationKind::Rectangle, AnnotationPosition::Rectangle(r)) => {
          let rect = norm_rect_to_screen(
            NormRect {
              x0: r.x0,
              y0: r.y0,
              x1: r.x1,
              y1: r.y1,
            },
            &self.transform,
          );
          painter.rect_stroke(
            egui_bridge::to_egui_rect(rect),
            0.0,
            stroke,
            egui::StrokeKind::Outside,
          );
        }
        (AnnotationKind::Arrow, AnnotationPosition::Arrow(a)) => {
          let p0 = norm_to_screen_pos2(NormPoint { x: a.x0, y: a.y0 }, &self.transform);
          let p1 = norm_to_screen_pos2(NormPoint { x: a.x1, y: a.y1 }, &self.transform);
          painter.line_segment(
            [egui_bridge::to_egui_pos(p0), egui_bridge::to_egui_pos(p1)],
            stroke,
          );
        }
        (AnnotationKind::Text, AnnotationPosition::Text(t)) => {
          let p = norm_to_screen_pos2(NormPoint { x: t.x, y: t.y }, &self.transform);
          let text = if ann.content.is_empty() {
            "备注"
          } else {
            &ann.content
          };
          painter.text(
            egui_bridge::to_egui_pos(p),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::proportional(14.0),
            color,
          );
        }
        _ => {}
      }
    }
  }

  pub fn sync_transform(&mut self, other: &ViewportTransform) {
    self.transform = *other;
  }
}

fn stroke_from_style(style: &AnnotationStyle) -> Stroke {
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
