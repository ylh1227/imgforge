//! Imatest 数据提取模块。

pub mod domain;
pub mod error;
pub mod parser;
pub mod service;

#[cfg(feature = "ocr")]
pub mod ocr;

#[cfg(feature = "gui")]
pub mod ui;

pub use domain::{
    BatchComparison, ComparisonRow, EvaluationResult, EvaluationStatus, EvaluationSummary,
    ExtractionBatch, ExtractionRecord, ImatestModule, MetricValue, OcrMetadata, ParseWarning,
    PassStatus, SourceKind, SummaryCell, SummaryColumn, SummaryRecordRef, SummaryRow, SummaryTable,
    ThresholdOp, ThresholdProfile, ThresholdRule, TrendStatus, UnmappedFieldStat,
};
pub use error::{DataExtractError, DataExtractResult};
pub use service::{
    CompareService, DataExportService, DataExtractService, ExportResult, SummaryService,
    ThresholdService,
};

#[cfg(feature = "gui")]
pub use ui::{DataExtractPanel, DataExtractPanelOutput};

#[cfg(test)]
mod tests;
