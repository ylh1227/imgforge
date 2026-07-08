//! OCR 前图像预处理。

use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::GenericImageView;
use image::{DynamicImage, Luma};

use crate::data_extract::error::{DataExtractError, DataExtractResult};

/// 转灰度并适度放大，提升 OCR 识别率。
pub fn preprocess_for_ocr(path: &Path) -> DataExtractResult<PathBuf> {
    let img = image::open(path).map_err(|e| DataExtractError::ParseFailed {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })?;

    let gray = DynamicImage::ImageLuma8(to_luma8(&img));
    let (w, h) = gray.dimensions();
    let scale = if w < 1200 { 2.0 } else { 1.0 };
    let resized = if scale > 1.0 {
        gray.resize(
            (w as f32 * scale) as u32,
            (h as f32 * scale) as u32,
            FilterType::Lanczos3,
        )
    } else {
        gray
    };

    let stable = tempfile::Builder::new()
        .prefix("imgforge_ocr_")
        .suffix(".png")
        .tempfile()
        .map_err(DataExtractError::Io)?;
    let stable_path = stable.path().to_path_buf();
    resized
        .save(&stable_path)
        .map_err(|e| DataExtractError::ParseFailed {
            path: path.to_path_buf(),
            detail: e.to_string(),
        })?;
    // 保持临时文件到进程结束
    let _ = stable.keep();
    Ok(stable_path)
}

fn to_luma8(img: &DynamicImage) -> image::GrayImage {
    let (w, h) = img.dimensions();
    let mut out = image::GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x, y);
            let l = Luma([(0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8]);
            out.put_pixel(x, y, l);
        }
    }
    out
}
