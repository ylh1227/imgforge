//! 单条提取记录。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::evaluation::EvaluationResult;
use super::imatest_module::ImatestModule;
use super::metric_value::MetricValue;
use super::ocr_metadata::OcrMetadata;
use super::parse_warning::ParseWarning;
use super::source_kind::SourceKind;

/// 一次测试结果中的单条指标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractionRecord {
    pub module: ImatestModule,
    pub metric_key: String,
    pub raw_name: String,
    pub value: MetricValue,
    pub sample_name: Option<String>,
    pub source_path: PathBuf,
    pub parser_name: String,
    #[serde(default)]
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<EvaluationResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr: Option<OcrMetadata>,
    pub warnings: Vec<ParseWarning>,
}

impl ExtractionRecord {
    pub fn new(
        module: ImatestModule,
        metric_key: impl Into<String>,
        raw_name: impl Into<String>,
        value: MetricValue,
        source_path: PathBuf,
        parser_name: impl Into<String>,
    ) -> Self {
        Self {
            module,
            metric_key: metric_key.into(),
            raw_name: raw_name.into(),
            value,
            sample_name: None,
            source_path,
            parser_name: parser_name.into(),
            source_kind: SourceKind::StructuredFile,
            evaluation: None,
            ocr: None,
            warnings: Vec::new(),
        }
    }

    pub fn with_sample(mut self, sample: impl Into<String>) -> Self {
        self.sample_name = Some(sample.into());
        self
    }

    pub fn with_source_kind(mut self, kind: SourceKind) -> Self {
        self.source_kind = kind;
        self
    }

    pub fn with_evaluation(mut self, evaluation: EvaluationResult) -> Self {
        self.evaluation = Some(evaluation);
        self
    }

    pub fn with_ocr(mut self, ocr: OcrMetadata) -> Self {
        self.source_kind = SourceKind::OcrImage;
        self.ocr = Some(ocr);
        self
    }

    pub fn with_warning(mut self, warning: ParseWarning) -> Self {
        self.warnings.push(warning);
        self
    }

    pub fn evaluation_status(&self) -> super::evaluation::EvaluationStatus {
        self.evaluation
            .as_ref()
            .map(|e| e.status)
            .unwrap_or_default()
    }
}
