//! 评审领域模型与纯逻辑。

pub(crate) mod annotation;
pub(crate) mod batch;
pub(crate) mod convert_params;
pub(crate) mod coords;
pub(crate) mod custom_status;
pub(crate) mod image_item;
pub(crate) mod metadata;
pub(crate) mod render;
pub(crate) mod tag;

pub use annotation::{
    Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle, ArrowPosition,
    RectanglePosition, TextPosition,
};
pub use batch::{BatchStats, ReviewBatch};
pub use convert_params::ConvertParams;
pub use coords::{
    norm_rect_to_screen, norm_to_pixel, norm_to_screen, pixel_to_norm, screen_rect_to_norm,
    screen_to_norm, NormPoint, NormRect, PixelPoint, PixelRect, ScreenPoint, ViewportTransform,
};
pub use custom_status::CustomStatusLabel;
pub use image_item::{AnnotationFilter, ImageFilter, ImageSortKey, ReviewImageItem, ReviewStatus};
pub use metadata::{format_bytes, read_image_metadata, ImageMetadata};
pub use render::{burn_annotations_onto, render_annotations_overlay, render_cache_key};
pub use tag::{ReviewTag, TagFilterMode};
