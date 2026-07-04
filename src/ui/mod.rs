//! UI 层：进度条、日志与报告。

pub mod doctor;
pub mod logger;
pub mod progress;
pub mod report;

pub use logger::init_logger;
pub use report::ProcessReport;
