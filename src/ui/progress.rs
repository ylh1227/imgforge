//! 进度报告抽象：终端进度条与 GUI 进度共用。

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use indicatif::{ProgressBar, ProgressStyle};

/// 任务进度回调（CLI / GUI 共用）。
pub trait ProgressReporter: Send + Sync {
  fn set_total(&self, total: usize);
  fn inc(&self, sizes: Option<(u64, u64)>);
  fn finish(&self);
  fn fraction(&self) -> f32;
  fn status_label(&self) -> Option<String> {
    None
  }
}

/// 封装 indicatif 进度条，支持速度、剩余时间与压缩率统计。
pub struct ProgressManager {
  bar: ProgressBar,
  input_bytes: AtomicU64,
  output_bytes: AtomicU64,
}

impl ProgressManager {
  pub fn new(total: usize) -> Self {
    let bar = ProgressBar::new(total as u64);
    bar.set_style(
      ProgressStyle::with_template(
        "[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {per_sec} ETA {eta} saved {msg}",
      )
      .expect("valid progress template")
      .progress_chars("█▓░"),
    );
    bar.set_message("0 B");

    Self {
      bar,
      input_bytes: AtomicU64::new(0),
      output_bytes: AtomicU64::new(0),
    }
  }
}

impl ProgressReporter for ProgressManager {
  fn set_total(&self, total: usize) {
    self.bar.set_length(total as u64);
  }

  fn inc(&self, sizes: Option<(u64, u64)>) {
    if let Some((input, output)) = sizes {
      self.input_bytes.fetch_add(input, Ordering::Relaxed);
      self.output_bytes.fetch_add(output, Ordering::Relaxed);
      let saved = self
        .input_bytes
        .load(Ordering::Relaxed)
        .saturating_sub(self.output_bytes.load(Ordering::Relaxed));
      self.bar.set_message(format_bytes(saved));
    }
    self.bar.inc(1);
  }

  fn finish(&self) {
    self.bar.finish_with_message("done");
  }

  fn fraction(&self) -> f32 {
    let len = self.bar.length().unwrap_or(0);
    if len == 0 {
      return 0.0;
    }
    self.bar.position() as f32 / len as f32
  }
}

/// GUI 可轮询的共享进度状态。
pub struct GuiProgress {
  pub completed: AtomicUsize,
  pub total: AtomicUsize,
  pub current_file: Mutex<String>,
  input_bytes: AtomicU64,
  output_bytes: AtomicU64,
}

impl GuiProgress {
  pub fn new() -> Self {
    Self {
      completed: AtomicUsize::new(0),
      total: AtomicUsize::new(0),
      current_file: Mutex::new(String::new()),
      input_bytes: AtomicU64::new(0),
      output_bytes: AtomicU64::new(0),
    }
  }

  pub fn fraction(&self) -> f32 {
    let total = self.total.load(Ordering::Relaxed);
    if total == 0 {
      return 0.0;
    }
    self.completed.load(Ordering::Relaxed) as f32 / total as f32
  }

  pub fn saved_bytes(&self) -> u64 {
    self
      .input_bytes
      .load(Ordering::Relaxed)
      .saturating_sub(self.output_bytes.load(Ordering::Relaxed))
  }
}

impl Default for GuiProgress {
  fn default() -> Self {
    Self::new()
  }
}

impl ProgressReporter for GuiProgress {
  fn set_total(&self, total: usize) {
    self.total.store(total, Ordering::Relaxed);
  }

  fn inc(&self, sizes: Option<(u64, u64)>) {
    if let Some((input, output)) = sizes {
      self.input_bytes.fetch_add(input, Ordering::Relaxed);
      self.output_bytes.fetch_add(output, Ordering::Relaxed);
    }
    self.completed.fetch_add(1, Ordering::Relaxed);
  }

  fn finish(&self) {}

  fn fraction(&self) -> f32 {
    let total = self.total.load(Ordering::Relaxed);
    if total == 0 {
      return 0.0;
    }
    self.completed.load(Ordering::Relaxed) as f32 / total as f32
  }

  fn status_label(&self) -> Option<String> {
    let total = self.total.load(Ordering::Relaxed);
    if total == 0 {
      return None;
    }
    let done = self.completed.load(Ordering::Relaxed);
    Some(format!("正在处理 {done} / {total} …"))
  }
}

fn format_bytes(bytes: u64) -> String {
  const KB: u64 = 1024;
  const MB: u64 = KB * 1024;
  const GB: u64 = MB * 1024;
  if bytes >= GB {
    format!("{:.2} GB", bytes as f64 / GB as f64)
  } else if bytes >= MB {
    format!("{:.2} MB", bytes as f64 / MB as f64)
  } else if bytes >= KB {
    format!("{:.2} KB", bytes as f64 / KB as f64)
  } else {
    format!("{bytes} B")
  }
}
