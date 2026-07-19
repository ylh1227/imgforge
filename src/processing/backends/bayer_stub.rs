//! 未启用 `bayer` feature 时的占位实现。
//!
//! 扩展名识别与完整实现保持一致，便于配对参考/路由在无 demosaic 时仍正确工作。

use std::path::Path;

use image::DynamicImage;

use crate::core::error::{AppError, AppResult};

/// 常见相机 RAW 扩展名（与 `bayer_codec` 一致）。
pub fn is_raw_camera_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "3fr"
            | "arw"
            | "cr2"
            | "cr3"
            | "crw"
            | "dcr"
            | "dng"
            | "erf"
            | "iiq"
            | "kdc"
            | "mef"
            | "mos"
            | "mrw"
            | "nef"
            | "nrw"
            | "orf"
            | "pef"
            | "raf"
            | "raw"
            | "rw2"
            | "rwl"
            | "sr2"
            | "srf"
    )
}

pub fn is_raw_camera_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(is_raw_camera_extension)
}

pub fn decode_bayer_only(_bytes: &[u8], path: &Path) -> AppResult<DynamicImage> {
    Err(AppError::Config(format!(
        "Bayer/RAW demosaic requires the `bayer` feature ({}). Rebuild with --features bayer.",
        path.display()
    )))
}
