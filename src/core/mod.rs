//! 基础设施层：错误、类型与处理上下文。

pub mod context;
pub mod error;
pub mod types;

pub use context::ImageContext;
pub use error::{AppError, AppResult};
pub use types::{
    AdjustOptions, BrightnessMatchMetric, BrightnessMatchMode, BrightnessMatchOptions, Concurrency,
    ImageFormat, MetadataPolicy, Quality, ResizeMode, ResizeOptions, ThumbnailSpec, Transform,
    WatermarkOptions, WatermarkPosition,
};
