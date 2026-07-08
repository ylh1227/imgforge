//! 处理报告生成与终端输出。

use std::path::PathBuf;
use std::time::Duration;

/// 单条失败记录。
#[derive(Debug, Clone)]
pub struct FailureRecord {
    pub path: PathBuf,
    pub error: String,
}

/// 批量处理统计报告。
#[derive(Debug, Clone)]
pub struct ProcessReport {
    pub scanned: usize,
    pub skipped: usize,
    pub total: usize,
    pub successes: usize,
    pub failures: Vec<FailureRecord>,
    pub elapsed: Duration,
    pub total_input_bytes: u64,
    pub total_output_bytes: u64,
    pub cancelled: bool,
}

impl ProcessReport {
    pub fn compression_ratio(&self) -> f64 {
        if self.total_input_bytes == 0 {
            return 0.0;
        }
        1.0 - (self.total_output_bytes as f64 / self.total_input_bytes as f64)
    }

    pub fn print_preview_summary(preview: &crate::io::batch_preview::BatchPreview, format: &str) {
        println!();
        println!("═══════════════════════════════════════");
        println!("  imgforge — Dry Run Preview");
        println!("═══════════════════════════════════════");
        for line in preview.summary_lines(format) {
            println!("  {line}");
        }
        if !preview.samples.is_empty() {
            println!();
            println!("  Sample outputs:");
            for s in &preview.samples {
                println!(
                    "    {} → {}",
                    s.input.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                    s.output.display()
                );
            }
        }
        if !preview.conflict_examples.is_empty() {
            println!();
            println!("  Conflicts:");
            for c in &preview.conflict_examples {
                println!("    • {c}");
            }
        }
        println!("═══════════════════════════════════════");
    }

    pub fn print_summary(&self) {
        println!();
        println!("═══════════════════════════════════════");
        println!("  imgforge — Processing Report");
        println!("═══════════════════════════════════════");
        println!("  Scanned:         {}", self.scanned);
        if self.skipped > 0 {
            println!("  Skipped:         {} (incremental)", self.skipped);
        }
        println!("  Processed:       {}", self.total);
        println!("  Succeeded:       {}", self.successes);
        println!("  Failed:          {}", self.failures.len());
        if self.cancelled {
            println!("  Status:          cancelled (partial results)");
        }
        println!(
            "  Elapsed:         {}",
            humantime::format_duration(self.elapsed)
        );
        println!(
            "  Input size:      {}",
            format_bytes(self.total_input_bytes)
        );
        println!(
            "  Output size:     {}",
            format_bytes(self.total_output_bytes)
        );
        println!(
            "  Compression:     {:.1}%",
            self.compression_ratio() * 100.0
        );

        if !self.failures.is_empty() {
            println!();
            println!("  Failures:");
            for f in &self.failures {
                println!("    • {} — {}", f.path.display(), f.error);
            }
        }
        println!("═══════════════════════════════════════");
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
