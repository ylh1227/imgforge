//! 数据提取领域模型。

pub mod comparison;
pub mod evaluation;
pub mod extraction_batch;
pub mod extraction_record;
pub mod imatest_module;
pub mod metric_value;
pub mod ocr_metadata;
pub mod parse_warning;
pub mod source_kind;
pub mod summary;
pub mod threshold;
pub mod unmapped;

pub use comparison::{BatchComparison, ComparisonRow, TrendStatus};
pub use evaluation::{EvaluationResult, EvaluationStatus, EvaluationSummary};
pub use extraction_batch::ExtractionBatch;
pub use extraction_record::ExtractionRecord;
pub use imatest_module::ImatestModule;
pub use metric_value::{MetricValue, PassStatus};
pub use ocr_metadata::OcrMetadata;
pub use parse_warning::ParseWarning;
pub use source_kind::SourceKind;
pub use summary::{
    SummaryCell, SummaryColumn, SummaryRecordRef, SummaryRow, SummaryTable, SummaryTableMode,
};
pub use threshold::{ThresholdOp, ThresholdProfile, ThresholdRule};
pub use unmapped::{collect_unmapped_fields, UnmappedFieldStat};
