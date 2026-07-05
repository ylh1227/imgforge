//! 标注实体：类型、归一化坐标、样式。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 标注类型（预留扩展位：新增变体 + match 分支即可）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum AnnotationKind {
  Rectangle = 0,
  Arrow = 1,
  Text = 2,
}

impl AnnotationKind {
  pub fn from_db(value: i32) -> Option<Self> {
    match value {
      0 => Some(Self::Rectangle),
      1 => Some(Self::Arrow),
      2 => Some(Self::Text),
      _ => None,
    }
  }

  pub fn db_value(self) -> i32 {
    self as i32
  }

  /// 持久化层 TEXT 列值（rect/arrow/text）。
  pub fn to_sql(self) -> &'static str {
    match self {
      Self::Rectangle => "rect",
      Self::Arrow => "arrow",
      Self::Text => "text",
    }
  }

  pub fn from_sql(value: &str) -> Option<Self> {
    match value {
      "rect" => Some(Self::Rectangle),
      "arrow" => Some(Self::Arrow),
      "text" => Some(Self::Text),
      _ => None,
    }
  }
}

/// 归一化坐标位置（0~1），JSON 持久化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationPosition {
  Rectangle(RectanglePosition),
  Arrow(ArrowPosition),
  Text(TextPosition),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectanglePosition {
  pub x0: f32,
  pub y0: f32,
  pub x1: f32,
  pub y1: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArrowPosition {
  pub x0: f32,
  pub y0: f32,
  pub x1: f32,
  pub y1: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextPosition {
  pub x: f32,
  pub y: f32,
}

/// 标注样式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationStyle {
  pub color: [u8; 4],
  pub line_width: f32,
}

impl Default for AnnotationStyle {
  fn default() -> Self {
    Self {
      color: [255, 59, 48, 255],
      line_width: 2.0,
    }
  }
}

/// 单条标注记录。
#[derive(Debug, Clone)]
pub struct Annotation {
  pub id: i64,
  pub image_item_id: i64,
  pub kind: AnnotationKind,
  pub position: AnnotationPosition,
  pub style: AnnotationStyle,
  pub content: String,
  pub created_at: DateTime<Utc>,
  pub locked: bool,
  pub z_index: i32,
}

impl Annotation {
  pub fn new_draft(
    image_item_id: i64,
    kind: AnnotationKind,
    position: AnnotationPosition,
    style: AnnotationStyle,
    content: String,
  ) -> Self {
    Self {
      id: 0,
      image_item_id,
      kind,
      position,
      style,
      content,
      created_at: chrono::Utc::now(),
      locked: false,
      z_index: 0,
    }
  }

  /// 标注在归一化坐标系下的焦点（用于画布定位）。
  pub fn focus_norm(&self) -> crate::review::domain::coords::NormPoint {
    use crate::review::domain::coords::NormPoint;
    match &self.position {
      AnnotationPosition::Rectangle(r) => NormPoint {
        x: (r.x0 + r.x1) * 0.5,
        y: (r.y0 + r.y1) * 0.5,
      },
      AnnotationPosition::Arrow(a) => NormPoint {
        x: (a.x0 + a.x1) * 0.5,
        y: (a.y0 + a.y1) * 0.5,
      },
      AnnotationPosition::Text(t) => NormPoint { x: t.x, y: t.y },
    }
  }
}
