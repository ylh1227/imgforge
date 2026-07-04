//! 程序入口：命令行模式。

mod cli;
mod cli_loader;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use eyre::WrapErr;
use imgforge::job::run_batch;
use imgforge::ui;

use crate::cli::{Cli, Commands};
use crate::cli_loader::load_config;

#[tokio::main]
async fn main() -> eyre::Result<()> {
  color_eyre::install().ok();

  let cli = Cli::parse();

  if let Some(Commands::Completions { shell }) = cli.command {
    Cli::generate_completions(shell.into());
    return Ok(());
  }

  if matches!(cli.command, Some(Commands::Doctor)) {
    ui::doctor::run_doctor();
    return Ok(());
  }

  ui::init_logger(cli.verbose);

  let config = load_config(&cli).wrap_err("failed to load configuration")?;

  tracing::info!(
    input = %config.input_dir.display(),
    output = %config.output_dir.display(),
    format = %config.target_format,
    concurrency = config.concurrency.value(),
    "starting imgforge"
  );

  let cancelled = Arc::new(AtomicBool::new(false));
  install_shutdown_handler(Arc::clone(&cancelled));

  if config.dry_run {
    tracing::info!("dry-run mode: no files will be written");
  }

  let report = run_batch(config, cancelled, None)
    .await
    .wrap_err("execution failed")?;

  report.print_summary();

  if !report.failures.is_empty() {
    std::process::exit(1);
  }

  Ok(())
}

/// 跨平台优雅退出：Unix Ctrl+C / Windows Ctrl+C 与 Ctrl+Break。
fn install_shutdown_handler(cancelled: Arc<AtomicBool>) {
  #[cfg(not(windows))]
  tokio::spawn(async move {
    if tokio::signal::ctrl_c().await.is_ok() {
      tracing::warn!("received Ctrl+C, shutting down gracefully...");
      cancelled.store(true, Ordering::Relaxed);
    }
  });

  #[cfg(windows)]
  tokio::spawn(async move {
    tokio::select! {
      _ = tokio::signal::ctrl_c() => {
        tracing::warn!("received Ctrl+C, shutting down gracefully...");
      }
      _ = tokio::signal::windows::ctrl_break() => {
        tracing::warn!("received Ctrl+Break, shutting down gracefully...");
      }
      _ = tokio::signal::windows::ctrl_close() => {
        tracing::warn!("received console close, shutting down gracefully...");
      }
    }
    cancelled.store(true, Ordering::Relaxed);
  });
}
