//! Imatest 结果解析器。

pub mod aliases;
pub mod csv_parser;
pub mod file_kind;
pub mod json_parser;
pub mod module_detector;
pub mod ocr_text_parser;
pub mod txt_parser;

use std::path::Path;

use crate::data_extract::domain::{ExtractionRecord, ParseWarning, SourceKind};
use crate::data_extract::error::DataExtractResult;
use crate::data_extract::parser::csv_parser::parse_csv_file;
use crate::data_extract::parser::file_kind::FileKind;
use crate::data_extract::parser::json_parser::parse_json_file;
use crate::data_extract::parser::txt_parser::{parse_html_file, parse_txt_file};

#[derive(Debug, Default)]
pub struct ParseFileOutput {
    pub records: Vec<ExtractionRecord>,
    pub warnings: Vec<ParseWarning>,
}

fn tag_source_kind(mut out: ParseFileOutput, kind: SourceKind) -> ParseFileOutput {
    for rec in &mut out.records {
        if rec.source_kind == SourceKind::StructuredFile && kind != SourceKind::StructuredFile {
            rec.source_kind = kind;
        } else if matches!(kind, SourceKind::TextFile | SourceKind::HtmlFile) {
            rec.source_kind = kind;
        }
    }
    out
}

/// 解析单个结果文件（不含 OCR 图片，图片由 extract_service 处理）。
pub fn parse_file(path: &Path) -> DataExtractResult<ParseFileOutput> {
    match FileKind::from_path(path) {
        FileKind::Csv => {
            let out = parse_csv_file(path)?;
            Ok(tag_source_kind(
                ParseFileOutput {
                    records: out.records,
                    warnings: out.warnings,
                },
                SourceKind::StructuredFile,
            ))
        }
        FileKind::Json => {
            let out = parse_json_file(path)?;
            Ok(tag_source_kind(
                ParseFileOutput {
                    records: out.records,
                    warnings: out.warnings,
                },
                SourceKind::StructuredFile,
            ))
        }
        FileKind::Txt => {
            let out = parse_txt_file(path)?;
            Ok(tag_source_kind(
                ParseFileOutput {
                    records: out.records,
                    warnings: out.warnings,
                },
                SourceKind::TextFile,
            ))
        }
        FileKind::Html => {
            let out = parse_html_file(path)?;
            Ok(tag_source_kind(
                ParseFileOutput {
                    records: out.records,
                    warnings: out.warnings,
                },
                SourceKind::HtmlFile,
            ))
        }
        FileKind::Image | FileKind::Unknown => Ok(ParseFileOutput::default()),
    }
}
