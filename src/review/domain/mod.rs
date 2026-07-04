//! 评审领域模型与纯逻辑。

pub(crate) mod annotation;
pub(crate) mod batch;
pub(crate) mod coords;
pub(crate) mod image_item;
pub(crate) mod render;

pub use annotation::{
  Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle, ArrowPosition,
  RectanglePosition, TextPosition,
};
pub use batch::{BatchStats, ReviewBatch};
pub use coords::{
  norm_rect_to_screen, norm_to_pixel, norm_to_screen, pixel_to_norm, screen_rect_to_norm,
  screen_to_norm, NormPoint, NormRect, PixelPoint, PixelRect, ScreenPoint, ViewportTransform,
};
pub use image_item::{ImageFilter, ReviewImageItem, ReviewStatus};
pub use render::{burn_annotations_onto, render_annotations_overlay, render_cache_key};
