//! Tesseract CLI 后端。

use std::path::Path;

use crate::data_extract::error::{DataExtractError, DataExtractResult};
use crate::data_extract::ocr::{OcrAvailability, OcrOutput};
use crate::process_util;

pub fn check_tesseract() -> OcrAvailability {
    match process_util::command("tesseract").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let first = ver.lines().next().unwrap_or("tesseract").to_string();
            OcrAvailability {
                tesseract_ok: true,
                detail: first,
            }
        }
        Ok(out) => OcrAvailability {
            tesseract_ok: false,
            detail: format!(
                "tesseract 退出码 {}：请安装 Tesseract OCR",
                out.status.code().unwrap_or(-1)
            ),
        },
        Err(e) => OcrAvailability {
            tesseract_ok: false,
            detail: format!("未找到 tesseract：{e}。请安装后加入 PATH"),
        },
    }
}

pub fn run_tesseract(image_path: &Path, lang: &str) -> DataExtractResult<OcrOutput> {
    let out = process_util::command("tesseract")
        .arg(image_path)
        .arg("stdout")
        .arg("-l")
        .arg(lang)
        .arg("--psm")
        .arg("6")
        .output()
        .map_err(|e| DataExtractError::Message(format!("tesseract 执行失败：{e}")))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(DataExtractError::Message(format!("tesseract 失败：{err}")));
    }

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(OcrOutput {
        text,
        confidence: None,
        engine: "tesseract".into(),
        language: lang.to_string(),
    })
}
