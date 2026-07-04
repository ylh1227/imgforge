//! libvips 后端：运行时探测 + 可选 rs-vips 实现。

use std::sync::OnceLock;

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::core::types::ImageFormat;
use crate::processing::backends::native_backend::NativeBackend;
use crate::processing::backends::ImageBackend;

static VIPS_READY: OnceLock<bool> = OnceLock::new();

/// 探测 libvips 是否可用。
pub fn probe_vips() -> Result<&'static str, String> {
  if *VIPS_READY.get_or_init(init_vips) {
    Ok("available")
  } else {
    Err("libvips init failed".into())
  }
}

fn init_vips() -> bool {
  #[cfg(feature = "vips")]
  {
    rs_vips::Vips::init("imgforge").is_ok()
  }
  #[cfg(not(feature = "vips"))]
  {
    false
  }
}

/// libvips 后端：可用时使用 vips 编解码，否则回退原生实现。
pub struct VipsBackend {
  fallback: NativeBackend,
  use_vips: bool,
}

impl VipsBackend {
  pub fn try_create() -> Option<Self> {
    let use_vips = *VIPS_READY.get_or_init(init_vips);
    if use_vips {
      tracing::info!("using libvips backend");
    } else {
      tracing::debug!("libvips unavailable; using native fallback");
    }
    Some(Self {
      fallback: NativeBackend::new(),
      use_vips,
    })
  }
}

impl ImageBackend for VipsBackend {
  fn name(&self) -> &'static str {
    if self.use_vips {
      "vips"
    } else {
      "vips (native fallback)"
    }
  }

  fn supported_formats(&self) -> &[ImageFormat] {
    self.fallback.supported_formats()
  }

  fn decode(&self, ctx: &mut ImageContext) -> AppResult<()> {
    if self.use_vips {
      #[cfg(feature = "vips")]
      {
        if let Ok(()) = vips_impl::decode(ctx) {
          return Ok(());
        }
        tracing::warn!("vips decode failed; falling back to native");
      }
    }
    self.fallback.decode(ctx)
  }

  fn encode(&self, ctx: &mut ImageContext) -> AppResult<()> {
    if self.use_vips {
      #[cfg(feature = "vips")]
      {
        if let Ok(()) = vips_impl::encode(ctx) {
          return Ok(());
        }
        tracing::warn!("vips encode failed; falling back to native");
      }
    }
    self.fallback.encode(ctx)
  }
}

pub fn try_create() -> Option<VipsBackend> {
  VipsBackend::try_create()
}

#[cfg(feature = "vips")]
mod vips_impl {
  use image::DynamicImage;
  use rs_vips::VipsImage;

  use crate::core::context::ImageContext;
  use crate::core::error::{AppError, AppResult};
  use crate::core::types::ImageFormat;

  pub fn decode(ctx: &mut ImageContext) -> AppResult<()> {
    let bytes = ctx.raw_bytes.as_ref().ok_or_else(|| AppError::Pipeline {
      step: "vips_decode".into(),
      reason: "no raw bytes".into(),
    })?;

    let vips_img = VipsImage::new_from_buffer(bytes, "").map_err(|e| AppError::DecodeFailed {
      path: ctx.source_path.clone(),
      reason: e.to_string(),
    })?;

    let memory = vips_img.write_to_memory().map_err(|e| AppError::DecodeFailed {
      path: ctx.source_path.clone(),
      reason: e.to_string(),
    })?;

    let width = vips_img.get_width() as u32;
    let height = vips_img.get_height() as u32;
    let bands = vips_img.get_bands() as usize;

    let image = match bands {
      4 => image::RgbaImage::from_raw(width, height, memory)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| decode_err(ctx, "invalid RGBA buffer"))?,
      3 => {
        let mut rgba = Vec::with_capacity(memory.len() / 3 * 4);
        for chunk in memory.chunks_exact(3) {
          rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
        }
        image::RgbaImage::from_raw(width, height, rgba)
          .map(DynamicImage::ImageRgba8)
          .ok_or_else(|| decode_err(ctx, "invalid RGB buffer"))?
      }
      other => {
        return Err(decode_err(
          ctx,
          &format!("unsupported band count: {other}"),
        ));
      }
    };

    ctx.image = Some(image);
    Ok(())
  }

  pub fn encode(ctx: &mut ImageContext) -> AppResult<()> {
    let image = ctx.image.as_ref().ok_or_else(|| AppError::Pipeline {
      step: "vips_encode".into(),
      reason: "no image".into(),
    })?;

    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    let raw = rgba.into_raw();

    let vips_img =
      VipsImage::new_from_memory(&raw, w as i32, h as i32, 4, rs_vips::enums::BandFormat::Uchar)
        .map_err(|e| encode_err(ctx.target_format, e.to_string()))?;

    let mut tmp = tempfile::NamedTempFile::new().map_err(|e| AppError::io("vips-tmp", e))?;
    let path = tmp.path().to_string_lossy().to_string();

    match ctx.target_format {
      ImageFormat::Jpeg => {
        vips_img
          .jpegsave(&path)
          .map_err(|e| encode_err(ctx.target_format, e.to_string()))?;
      }
      ImageFormat::Png => {
        vips_img
          .pngsave(&path)
          .map_err(|e| encode_err(ctx.target_format, e.to_string()))?;
      }
      ImageFormat::WebP => {
        vips_img
          .webpsave(&path)
          .map_err(|e| encode_err(ctx.target_format, e.to_string()))?;
      }
      other => {
        return Err(AppError::EncodeFailed {
          format: other.to_string(),
          reason: "format not supported by vips fast path".into(),
        });
      }
    }

    let buffer = std::fs::read(tmp.path()).map_err(|e| AppError::io(tmp.path(), e))?;
    ctx.encoded_bytes = Some(buffer);
    ctx.output_size = ctx.encoded_bytes.as_ref().map(|b| b.len() as u64).unwrap_or(0);
    Ok(())
  }

  fn decode_err(ctx: &ImageContext, reason: &str) -> AppError {
    AppError::DecodeFailed {
      path: ctx.source_path.clone(),
      reason: reason.into(),
    }
  }

  fn encode_err(format: ImageFormat, reason: String) -> AppError {
    AppError::EncodeFailed { format: format.to_string(), reason }
  }
}
