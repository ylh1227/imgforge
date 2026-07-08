//! JPEG XL 编解码（feature: jpegxl）。

use image::{DynamicImage, RgbaImage};
use jxl_oxide::JxlImage;
use zune_core::bit_depth::BitDepth;
use zune_core::colorspace::ColorSpace;
use zune_core::options::EncoderOptions;
use zune_jpegxl::JxlSimpleEncoder;

use crate::core::error::{AppError, AppResult};

/// 使用 jxl-oxide 解码 JPEG XL。
pub fn decode_jpegxl(bytes: &[u8]) -> AppResult<DynamicImage> {
    let image = JxlImage::builder()
        .read(bytes)
        .map_err(|e| AppError::DecodeFailed {
            path: "jxl".into(),
            reason: e.to_string(),
        })?;

    let render = image.render_frame(0).map_err(|e| AppError::DecodeFailed {
        path: "jxl".into(),
        reason: e.to_string(),
    })?;

    let mut stream = render.stream();
    let width = stream.width() as usize;
    let height = stream.height() as usize;
    let channels = stream.channels() as usize;
    let mut samples = vec![0u8; width * height * channels];
    stream.write_to_buffer(&mut samples);

    match channels {
        4 => RgbaImage::from_raw(width as u32, height as u32, samples)
            .map(DynamicImage::ImageRgba8)
            .ok_or_else(|| AppError::DecodeFailed {
                path: "jxl".into(),
                reason: "failed to build RGBA image".into(),
            }),
        3 => {
            let mut rgba = Vec::with_capacity(width * height * 4);
            for chunk in samples.chunks_exact(3) {
                rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            RgbaImage::from_raw(width as u32, height as u32, rgba)
                .map(DynamicImage::ImageRgba8)
                .ok_or_else(|| AppError::DecodeFailed {
                    path: "jxl".into(),
                    reason: "failed to build RGBA image from RGB".into(),
                })
        }
        other => Err(AppError::DecodeFailed {
            path: "jxl".into(),
            reason: format!("unsupported channel count: {other}"),
        }),
    }
}

/// 使用 zune-jpegxl 编码 JPEG XL。
pub fn encode_jpegxl(image: &DynamicImage, quality: u8) -> AppResult<Vec<u8>> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let raw = rgba.into_raw();

    let effort = if quality >= 95 {
        7
    } else if quality >= 85 {
        5
    } else {
        4
    };
    let options = EncoderOptions::new(
        width as usize,
        height as usize,
        ColorSpace::RGB,
        BitDepth::Eight,
    )
    .set_quality(quality)
    .set_effort(effort);
    let encoder = JxlSimpleEncoder::new(&raw, options);
    let mut output = Vec::new();
    encoder
        .encode(&mut output)
        .map_err(|e| encode_err(e.to_string()))?;
    Ok(output)
}

fn encode_err(reason: String) -> AppError {
    AppError::EncodeFailed {
        format: "jxl".into(),
        reason,
    }
}
