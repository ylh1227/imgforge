//! 按目标文件大小拟合有损编码质量（二分搜索）。

use image::DynamicImage;

use crate::core::error::AppResult;
use crate::core::types::{ImageFormat, Quality};
use crate::processing::backends::native_backend::encode_dynamic_image;

/// 支持按质量调节的有损格式。
pub fn supports_quality_target(format: ImageFormat) -> bool {
    match format {
        ImageFormat::Jpeg | ImageFormat::WebP => true,
        #[cfg(feature = "avif")]
        ImageFormat::Avif => true,
        #[cfg(feature = "jpegxl")]
        ImageFormat::JpegXl => true,
        _ => false,
    }
}

/// 二分搜索满足 `encoded_size <= max_bytes` 的最高质量；失败时返回最低可用质量。
pub fn fit_quality_to_max_bytes(
    image: &DynamicImage,
    format: ImageFormat,
    max_bytes: u64,
) -> AppResult<Quality> {
    if max_bytes == 0 {
        return Quality::new(1);
    }

    let mut lo: u8 = 1;
    let mut hi: u8 = 100;
    let mut best = 1u8;

    while lo <= hi {
        let mid = (lo + hi) / 2;
        let q = Quality::new(mid)?;
        let bytes = encode_dynamic_image(image, format, q)?;
        if bytes.len() as u64 <= max_bytes {
            best = mid;
            lo = mid.saturating_add(1);
        } else {
            hi = mid.saturating_sub(1);
        }
    }

    Quality::new(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn fits_under_max_bytes() {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(64, 64));
        let q = fit_quality_to_max_bytes(&img, ImageFormat::WebP, 4_000).unwrap();
        let bytes = encode_dynamic_image(&img, ImageFormat::WebP, q).expect("encode");
        assert!(bytes.len() as u64 <= 4_000);
    }
}
