//! 数据提取服务层。

pub mod compare_service;
pub mod export_service;
pub mod extract_service;
pub mod insight_service;
pub mod query_service;
pub mod scanner;
pub mod summary_service;
pub mod threshold_service;

pub use compare_service::CompareService;
pub use export_service::{DataExportService, ExportResult, TableExportColumn, TableExportSchema};
pub use extract_service::DataExtractService;
pub use insight_service::{DataInsightReport, DataInsightService, InsightCount, OutlierInsight};
pub use query_service::{DataQuery, DataQueryService};
pub use summary_service::SummaryService;
pub use threshold_service::ThresholdService;
