//! 未启用 `bayer` feature 时的占位实现。

use std::path::Path;

use image::DynamicImage;

use crate::core::error::{AppError, AppResult};

pub fn is_raw_camera_extension(ext: &str) -> bool {
    let _ = ext;
    false
}

pub fn is_raw_camera_path(path: &Path) -> bool {
    let _ = path;
    false
}

pub fn decode_bayer_only(_bytes: &[u8], path: &Path) -> AppResult<DynamicImage> {
    Err(AppError::Config(format!(
        "Bayer/RAW demosaic requires the `bayer` feature ({}). Rebuild with --features bayer.",
        path.display()
    )))
}
