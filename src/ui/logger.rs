//! 结构化日志初始化。

use tracing_subscriber::{fmt, EnvFilter};

/// 根据环境变量 `RUST_LOG` 初始化 tracing 日志。
pub fn init_logger(verbose: bool) {
  let default_level = if verbose { "debug" } else { "info" };
  let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| EnvFilter::new(default_level));

  fmt()
    .with_env_filter(filter)
    .with_target(false)
    .with_thread_ids(false)
    .init();
}
