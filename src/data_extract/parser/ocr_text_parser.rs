//! OCR 文本清洗与解析。

use std::path::Path;

use crate::data_extract::domain::{OcrMetadata, SourceKind};
use crate::data_extract::error::DataExtractResult;
use crate::data_extract::parser::txt_parser::parse_txt_content;
use crate::data_extract::parser::ParseFileOutput;

/// 清洗 OCR 常见噪声。
pub fn clean_ocr_text(text: &str) -> String {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.replace('|', " ").replace('—', "-"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_ocr_text(
    path: &Path,
    raw_text: &str,
    ocr: OcrMetadata,
) -> DataExtractResult<ParseFileOutput> {
    let cleaned = clean_ocr_text(raw_text);
    let parsed = parse_txt_content(path, &cleaned)?;
    let mut out = ParseFileOutput {
        records: parsed.records,
        warnings: parsed.warnings,
    };
    for rec in &mut out.records {
        rec.source_kind = SourceKind::OcrImage;
        rec.parser_name = "ocr".into();
        rec.ocr = Some(ocr.clone());
        if ocr.confidence.is_some_and(|c| c < 60.0) {
            rec.warnings
                .push(crate::data_extract::domain::ParseWarning::new(
                    "low_ocr_confidence",
                    format!("OCR 置信度偏低：{:.1}", ocr.confidence.unwrap_or(0.0)),
                ));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_pipe_noise() {
        let t = clean_ocr_text("MTF50 | 0.42\n\nSNR (dB): 36.5");
        assert!(t.contains("MTF50"));
        assert!(!t.contains('|'));
    }
}
