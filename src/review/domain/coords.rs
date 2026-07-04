//! 屏幕坐标 ↔ 归一化坐标 ↔ 原图像素坐标 纯函数换算（无 UI 框架依赖）。

/// 二维向量。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
  pub x: f32,
  pub y: f32,
}

/// 二维点。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pos2 {
  pub x: f32,
  pub y: f32,
}

impl Pos2 {
  pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

  pub fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }
}

/// 矩形区域。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
  pub min: Pos2,
  pub size: Vec2,
}

impl Rect {
  pub fn from_min_size(min: Pos2, size: Vec2) -> Self {
    Self { min, size }
  }

  pub fn from_two_pos(a: Pos2, b: Pos2) -> Self {
    let min = Pos2 {
      x: a.x.min(b.x),
      y: a.y.min(b.y),
    };
    let max = Pos2 {
      x: a.x.max(b.x),
      y: a.y.max(b.y),
    };
    Self {
      min,
      size: Vec2 {
        x: max.x - min.x,
        y: max.y - min.y,
      },
    }
  }

  pub fn center(self) -> Pos2 {
    Pos2 {
      x: self.min.x + self.size.x * 0.5,
      y: self.min.y + self.size.y * 0.5,
    }
  }

  pub fn width(self) -> f32 {
    self.size.x
  }

  pub fn height(self) -> f32 {
    self.size.y
  }
}

/// 归一化点（0~1，相对原图宽高）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormPoint {
  pub x: f32,
  pub y: f32,
}

/// 归一化矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormRect {
  pub x0: f32,
  pub y0: f32,
  pub x1: f32,
  pub y1: f32,
}

/// 屏幕/画布坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
  pub x: f32,
  pub y: f32,
}

/// 原图像素坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelPoint {
  pub x: i32,
  pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
  pub x0: i32,
  pub y0: i32,
  pub x1: i32,
  pub y1: i32,
}

/// 画布视口变换：平移 + 缩放 + 图片在画布中的基础矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportTransform {
  pub pan: Vec2,
  pub zoom: f32,
  pub image_rect: Rect,
}

impl ViewportTransform {
  pub fn fit_image(canvas_size: Vec2, image_size: (u32, u32)) -> Self {
    let iw = image_size.0 as f32;
    let ih = image_size.1 as f32;
    if iw <= 0.0 || ih <= 0.0 {
      return Self {
        pan: Vec2::ZERO,
        zoom: 1.0,
        image_rect: Rect::from_min_size(Pos2::ZERO, canvas_size),
      };
    }
    let scale = (canvas_size.x / iw).min(canvas_size.y / ih);
    let display = Vec2::new(iw * scale, ih * scale);
    let origin = Pos2::new(
      (canvas_size.x - display.x) * 0.5,
      (canvas_size.y - display.y) * 0.5,
    );
    Self {
      pan: Vec2::ZERO,
      zoom: 1.0,
      image_rect: Rect::from_min_size(origin, display),
    }
  }

  pub fn one_to_one(canvas_size: Vec2, image_size: (u32, u32)) -> Self {
    let iw = image_size.0 as f32;
    let ih = image_size.1 as f32;
    let origin = Pos2::new((canvas_size.x - iw) * 0.5, (canvas_size.y - ih) * 0.5);
    Self {
      pan: Vec2::ZERO,
      zoom: 1.0,
      image_rect: Rect::from_min_size(origin, Vec2::new(iw, ih)),
    }
  }

  pub fn displayed_image_rect(self) -> Rect {
    let center = self.image_rect.center();
    let size = Vec2 {
      x: self.image_rect.size.x * self.zoom,
      y: self.image_rect.size.y * self.zoom,
    };
    let min = Pos2 {
      x: center.x - size.x * 0.5 + self.pan.x,
      y: center.y - size.y * 0.5 + self.pan.y,
    };
    Rect::from_min_size(min, size)
  }
}

impl Vec2 {
  pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

  pub fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }
}

pub fn screen_to_norm(screen: ScreenPoint, transform: &ViewportTransform) -> NormPoint {
  let rect = transform.displayed_image_rect();
  let nx = ((screen.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
  let ny = ((screen.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
  NormPoint { x: nx, y: ny }
}

pub fn norm_to_screen(norm: NormPoint, transform: &ViewportTransform) -> ScreenPoint {
  let rect = transform.displayed_image_rect();
  ScreenPoint {
    x: rect.min.x + norm.x * rect.width(),
    y: rect.min.y + norm.y * rect.height(),
  }
}

pub fn norm_to_screen_pos2(norm: NormPoint, transform: &ViewportTransform) -> Pos2 {
  let s = norm_to_screen(norm, transform);
  Pos2 { x: s.x, y: s.y }
}

pub fn norm_to_pixel(norm: NormPoint, image_size: (u32, u32)) -> PixelPoint {
  PixelPoint {
    x: (norm.x * image_size.0 as f32).round() as i32,
    y: (norm.y * image_size.1 as f32).round() as i32,
  }
}

pub fn pixel_to_norm(pixel: PixelPoint, image_size: (u32, u32)) -> NormPoint {
  let w = image_size.0.max(1) as f32;
  let h = image_size.1.max(1) as f32;
  NormPoint {
    x: (pixel.x as f32 / w).clamp(0.0, 1.0),
    y: (pixel.y as f32 / h).clamp(0.0, 1.0),
  }
}

pub fn screen_rect_to_norm(a: ScreenPoint, b: ScreenPoint, transform: &ViewportTransform) -> NormRect {
  let n0 = screen_to_norm(a, transform);
  let n1 = screen_to_norm(b, transform);
  NormRect {
    x0: n0.x.min(n1.x),
    y0: n0.y.min(n1.y),
    x1: n0.x.max(n1.x),
    y1: n0.y.max(n1.y),
  }
}

pub fn norm_rect_to_screen(rect: NormRect, transform: &ViewportTransform) -> Rect {
  let p0 = norm_to_screen_pos2(NormPoint { x: rect.x0, y: rect.y0 }, transform);
  let p1 = norm_to_screen_pos2(NormPoint { x: rect.x1, y: rect.y1 }, transform);
  Rect::from_two_pos(p0, p1)
}

#[cfg(feature = "gui")]
pub mod egui_bridge {
  use super::*;

  pub fn vec2(v: eframe::egui::Vec2) -> Vec2 {
    Vec2 { x: v.x, y: v.y }
  }

  pub fn to_egui_pos(p: Pos2) -> eframe::egui::Pos2 {
    eframe::egui::Pos2::new(p.x, p.y)
  }

  pub fn to_egui_rect(r: Rect) -> eframe::egui::Rect {
    eframe::egui::Rect::from_min_size(to_egui_pos(r.min), eframe::egui::vec2(r.size.x, r.size.y))
  }

  pub fn from_egui_pos(p: eframe::egui::Pos2) -> Pos2 {
    Pos2 { x: p.x, y: p.y }
  }

  pub fn from_egui_vec(v: eframe::egui::Vec2) -> Vec2 {
    Vec2 { x: v.x, y: v.y }
  }
}
