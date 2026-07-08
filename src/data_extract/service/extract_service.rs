//! 提取服务：扫描 + 解析 + 聚合 + 阈值判定。

use std::path::Path;

use crate::data_extract::domain::{ExtractionBatch, OcrMetadata, ThresholdProfile};
use crate::data_extract::error::DataExtractResult;
use crate::data_extract::parser::ocr_text_parser::parse_ocr_text;
use crate::data_extract::parser::parse_file;
use crate::data_extract::service::scanner::scan_directory;

#[cfg(feature = "ocr")]
use crate::data_extract::ocr::{recognize_image, OcrOutput};

pub struct DataExtractService;

impl DataExtractService {
    /// 从目录或单文件导入并解析，可选应用阈值。
    pub fn extract_from_path(path: &Path) -> DataExtractResult<ExtractionBatch> {
        Self::extract_with_thresholds(path, None)
    }

    pub fn extract_with_thresholds(
        path: &Path,
        thresholds: Option<&ThresholdProfile>,
    ) -> DataExtractResult<ExtractionBatch> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Imatest 结果".to_string());

        let mut batch = ExtractionBatch::new(name, path.to_path_buf());
        let files = scan_directory(path);
        batch.files_scanned = files.len();

        for file in &files {
            #[cfg(not(feature = "ocr"))]
            if crate::data_extract::parser::file_kind::is_image_file(file) {
                batch
                    .warnings
                    .push(crate::data_extract::domain::ParseWarning::new(
                        "ocr_unavailable",
                        format!("跳过图片（未启用 OCR）：{}", file.display()),
                    ));
                continue;
            }

            let parsed = if crate::data_extract::parser::file_kind::is_image_file(file) {
                Self::parse_image_file(file)
            } else {
                parse_file(file)
            };

            match parsed {
                Ok(out) => {
                    if !out.records.is_empty() {
                        batch.files_parsed += 1;
                    }
                    batch.records.extend(out.records);
                    batch.warnings.extend(out.warnings);
                }
                Err(e) => {
                    batch
                        .warnings
                        .push(crate::data_extract::domain::ParseWarning::new(
                            "parse_error",
                            format!("{}: {e}", file.display()),
                        ));
                }
            }
        }

        if let Some(profile) = thresholds {
            batch.apply_thresholds(profile);
        } else {
            batch.refresh_unmapped();
        }

        Ok(batch)
    }

    fn parse_image_file(
        path: &Path,
    ) -> DataExtractResult<crate::data_extract::parser::ParseFileOutput> {
        #[cfg(feature = "ocr")]
        {
            let OcrOutput {
                text,
                confidence,
                engine,
                language,
            } = recognize_image(path, "eng+chi_sim")?;
            let cache_path = write_ocr_cache(path, &text)?;
            let meta = OcrMetadata::new(engine, language)
                .with_text_cache(cache_path)
                .with_confidence(confidence.unwrap_or(75.0));
            return parse_ocr_text(path, &text, meta);
        }
        #[cfg(not(feature = "ocr"))]
        {
            let _ = path;
            Ok(crate::data_extract::parser::ParseFileOutput::default())
        }
    }
}

#[cfg(feature = "ocr")]
fn write_ocr_cache(image_path: &Path, text: &str) -> DataExtractResult<std::path::PathBuf> {
    let stem = image_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "ocr".into());
    let cache_dir = crate::data_extract::service::threshold_service::thresholds_path()
        .parent()
        .map(|p| p.join("ocr_cache"))
        .unwrap_or_else(|| std::path::PathBuf::from("ocr_cache"));
    std::fs::create_dir_all(&cache_dir)?;
    let out = cache_dir.join(format!("{stem}.ocr.txt"));
    std::fs::write(&out, text)?;
    Ok(out)
}
