//! 图像处理后端抽象与实现。

pub mod native_backend;

#[cfg(feature = "avif")]
pub mod avif_codec;
#[cfg(feature = "jpegxl")]
pub mod jxl_codec;

#[cfg(feature = "vips")]
mod vips_backend;

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::core::types::ImageFormat;

/// 统一编解码后端接口，实现后端无关性。
pub trait ImageBackend: Send + Sync {
  fn name(&self) -> &'static str;
  fn decode(&self, ctx: &mut ImageContext) -> AppResult<()>;
  fn encode(&self, ctx: &mut ImageContext) -> AppResult<()>;
  fn supported_formats(&self) -> &[ImageFormat];
}

/// 根据 feature 选择默认后端。
pub fn default_backend() -> Box<dyn ImageBackend> {
  #[cfg(feature = "vips")]
  {
    if let Some(backend) = vips_backend::try_create() {
      return Box::new(backend);
    }
  }
  Box::new(native_backend::NativeBackend::new())
}
