//! 调度层：任务定义与混合并发执行。

pub mod executor;
pub mod task;

pub use executor::{ExecutionResult, Executor};
pub use task::ConversionTask;
