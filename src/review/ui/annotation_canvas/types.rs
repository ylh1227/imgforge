//! 画布专用类型别名与工具枚举（复用 domain 层实体）。

pub use crate::review::domain::annotation::{
  Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle,
};
pub use crate::review::domain::coords::NormRect;

/// 标注类型别名（与 domain `AnnotationKind` 一致）。
pub type AnnotationType = AnnotationKind;

/// 归一化矩形别名（与 domain `NormRect` 一致）。
pub type NormalizedRect = NormRect;

/// 画布工具：选择 / 矩形 / 箭头 / 文字。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasTool {
  #[default]
  Select,
  Rectangle,
  Arrow,
  Text,
}

impl CanvasTool {
  pub fn label(self) -> &'static str {
    match self {
      Self::Select => "选择",
      Self::Rectangle => "矩形",
      Self::Arrow => "箭头",
      Self::Text => "文字",
    }
  }
}

/// 创建标注时的草稿载荷（无数据库 id，由上层分配）。
#[derive(Debug, Clone)]
pub struct AnnotationDraft {
  pub kind: AnnotationKind,
  pub position: AnnotationPosition,
  pub style: AnnotationStyle,
  pub content: String,
}

/// 用于撤销栈匹配刚创建的标注（归一化坐标指纹）。
#[derive(Debug, Clone)]
pub(crate) struct AnnotationFingerprint {
  pub kind: AnnotationKind,
  pub position: AnnotationPosition,
}

impl AnnotationFingerprint {
  pub fn from_parts(kind: AnnotationKind, position: &AnnotationPosition) -> Self {
    Self {
      kind,
      position: position.clone(),
    }
  }

  pub fn matches(&self, ann: &Annotation) -> bool {
    ann.kind == self.kind && positions_close(&ann.position, &self.position)
  }
}

fn positions_close(a: &AnnotationPosition, b: &AnnotationPosition) -> bool {
  const EPS: f32 = 0.002;
  match (a, b) {
    (
      AnnotationPosition::Rectangle(ra),
      AnnotationPosition::Rectangle(rb),
    ) => {
      (ra.x0 - rb.x0).abs() < EPS
        && (ra.y0 - rb.y0).abs() < EPS
        && (ra.x1 - rb.x1).abs() < EPS
        && (ra.y1 - rb.y1).abs() < EPS
    }
    (AnnotationPosition::Arrow(ra), AnnotationPosition::Arrow(rb)) => {
      (ra.x0 - rb.x0).abs() < EPS
        && (ra.y0 - rb.y0).abs() < EPS
        && (ra.x1 - rb.x1).abs() < EPS
        && (ra.y1 - rb.y1).abs() < EPS
    }
    (AnnotationPosition::Text(ta), AnnotationPosition::Text(tb)) => {
      (ta.x - tb.x).abs() < EPS && (ta.y - tb.y).abs() < EPS
    }
    _ => false,
  }
}
