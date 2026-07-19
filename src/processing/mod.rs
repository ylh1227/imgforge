//! 图像处理层：流水线、步骤与后端。

pub mod backends;
pub mod brightness_match;
pub mod camera_match;
pub mod image_quality;
pub mod pipeline;
pub mod quality_fit;
pub mod steps;

pub use brightness_match::BrightnessMatchCache;
pub use pipeline::{build_default_pipeline, ProcessStep, ProcessingPipeline};
