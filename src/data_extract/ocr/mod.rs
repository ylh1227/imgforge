//! OCR 图像识别。

#[cfg(feature = "ocr")]
pub mod preprocess;
#[cfg(feature = "ocr")]
pub mod tesseract;

use std::path::Path;

use crate::data_extract::error::{DataExtractError, DataExtractResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrAvailability {
    pub tesseract_ok: bool,
    pub detail: String,
}

pub fn check_availability() -> OcrAvailability {
    #[cfg(feature = "ocr")]
    {
        return tesseract::check_tesseract();
    }
    #[cfg(not(feature = "ocr"))]
    {
        OcrAvailability {
            tesseract_ok: false,
            detail: "未编译 OCR 功能（需启用 ocr feature）".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OcrOutput {
    pub text: String,
    pub confidence: Option<f32>,
    pub engine: String,
    pub language: String,
}

/// 对图片执行 OCR。
pub fn recognize_image(path: &Path, lang: &str) -> DataExtractResult<OcrOutput> {
    #[cfg(feature = "ocr")]
    {
        let avail = check_availability();
        if !avail.tesseract_ok {
            return Err(DataExtractError::Message(avail.detail));
        }
        let processed = preprocess::preprocess_for_ocr(path)?;
        tesseract::run_tesseract(&processed, lang)
    }
    #[cfg(not(feature = "ocr"))]
    {
        let _ = (path, lang);
        Err(DataExtractError::Message(
            "OCR 未编译（需启用 ocr feature）".into(),
        ))
    }
}
