//! 混合并发执行引擎：tokio 异步 IO + rayon CPU 并行。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::config::app_config::AppConfig;
use crate::core::context::ImageContext;
use crate::core::error::{AppError, AppResult};
use crate::io::atomic_write::{atomic_write, validate_output_path};
use crate::io::incremental::IncrementalProcessor;
use crate::processing::pipeline::ProcessingPipeline;
use crate::scheduler::task::ConversionTask;
use crate::ui::progress::{ProgressManager, ProgressReporter};
use crate::ui::report::{FailureRecord, ProcessReport};

/// 执行结果。
pub struct ExecutionResult {
    pub report: ProcessReport,
}

/// 混合并发执行器。
pub struct Executor {
    config: AppConfig,
    cancelled: Arc<AtomicBool>,
}

impl Executor {
    pub fn new(config: AppConfig, cancelled: Arc<AtomicBool>) -> Self {
        Self { config, cancelled }
    }

    /// 执行批量转换任务。
    pub async fn run(
        &self,
        tasks: Vec<ConversionTask>,
        progress: Option<Arc<dyn ProgressReporter>>,
    ) -> AppResult<ExecutionResult> {
        let start = Instant::now();
        let pipeline = Arc::new(crate::processing::pipeline::build_pipeline(&self.config));
        let mut incremental = IncrementalProcessor::load(
            self.config.output_dir.join(".imgforge-state.toml"),
            self.config.incremental,
        )?;

        let scanned = tasks.len();
        let filter = incremental.filter_tasks(tasks)?;
        let skipped = filter.skipped;
        let tasks = filter.tasks;
        let total = tasks.len();

        let progress_reporter: Arc<dyn ProgressReporter> = match progress {
            Some(reporter) => reporter,
            None => Arc::new(ProgressManager::new(total)),
        };
        progress_reporter.set_total(total);

        // 大图全量读入内存时，按输入体积动态收紧并发，降低峰值 RSS。
        let max_input = tasks.iter().map(|t| t.input_size).max().unwrap_or(0);
        let configured = self.config.concurrency.value();
        let effective = effective_concurrency(configured, max_input, total);
        if effective < configured {
            tracing::info!(
                configured,
                effective,
                max_input_bytes = max_input,
                "reduced concurrency for large inputs"
            );
        }
        let semaphore = Arc::new(Semaphore::new(effective));

        if total == 0 {
            tracing::info!(scanned, skipped, "no tasks to process after filtering");
        }

        let mut successes = 0usize;
        let mut failures = Vec::new();
        let mut total_input_bytes = 0u64;
        let mut total_output_bytes = 0u64;

        let mut join_set: JoinSet<(ConversionTask, AppResult<TaskOutcome>)> = JoinSet::new();

        for task in tasks {
            if self.cancelled.load(Ordering::Relaxed) {
                break;
            }

            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| AppError::Cancelled)?;

            let pipeline = Arc::clone(&pipeline);
            let config = self.config.clone();
            let cancelled = Arc::clone(&self.cancelled);
            let progress = Arc::clone(&progress_reporter);

            join_set.spawn(async move {
                let result = process_single_task(task.clone(), &config, pipeline, &cancelled).await;
                progress.inc(
                    result
                        .as_ref()
                        .ok()
                        .map(|o| (task.input_size, o.output_size)),
                );
                drop(permit);
                (task, result)
            });
        }

        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok((task, Ok(outcome))) => {
                    successes += 1;
                    total_input_bytes += task.input_size;
                    total_output_bytes += outcome.output_size;
                    let _ = incremental.record_success(&task);
                }
                Ok((task, Err(e))) => {
                    failures.push(FailureRecord {
                        path: task.input_path.clone(),
                        error: e.to_string(),
                    });
                }
                Err(e) => {
                    failures.push(FailureRecord {
                        path: PathBuf::from("<unknown>"),
                        error: e.to_string(),
                    });
                }
            }
        }

        progress_reporter.finish();
        let _ = incremental.save();

        let report = ProcessReport {
            scanned,
            skipped,
            total,
            successes,
            failures: failures.clone(),
            elapsed: start.elapsed(),
            total_input_bytes,
            total_output_bytes,
            cancelled: self.cancelled.load(Ordering::Relaxed),
        };

        Ok(ExecutionResult { report })
    }
}

struct TaskOutcome {
    output_size: u64,
}

async fn process_single_task(
    task: ConversionTask,
    config: &AppConfig,
    pipeline: Arc<ProcessingPipeline>,
    cancelled: &AtomicBool,
) -> AppResult<TaskOutcome> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }

    let raw_bytes = fs::read(&task.input_path)
        .await
        .map_err(|e| AppError::io(&task.input_path, e))?;

    validate_output_path(&config.output_dir, &task.output_path)?;

    let mut ctx = ImageContext::new(
        task.input_path.clone(),
        task.output_path.clone(),
        task.format_override.unwrap_or(config.target_format),
        task.quality_override.unwrap_or(config.quality),
        task.input_size,
    );
    ctx.raw_bytes = Some(raw_bytes);
    ctx.resize = task.resize_override.unwrap_or(config.resize);
    ctx.adjust = config.adjust;
    ctx.metadata_policy = config.metadata_policy;
    ctx.transform = config.transform;
    ctx.watermark = config.watermark.clone();
    ctx.dry_run = config.dry_run;
    ctx.bayer_only = config.bayer_only;

    // CPU 密集流水线在阻塞线程池中执行，避免阻塞 tokio 运行时
    let result = tokio::task::spawn_blocking(move || {
        let r = pipeline.execute(&mut ctx);
        (r, ctx)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;

    let (pipeline_result, mut ctx) = result;
    pipeline_result?;

    if let Some(max_bytes) = config.target_max_bytes {
        if let Some(ref image) = ctx.image {
            if crate::processing::quality_fit::supports_quality_target(ctx.target_format) {
                let fitted = crate::processing::quality_fit::fit_quality_to_max_bytes(
                    image,
                    ctx.target_format,
                    max_bytes,
                )?;
                let encoded = crate::processing::backends::native_backend::encode_dynamic_image(
                    image,
                    ctx.target_format,
                    fitted,
                )?;
                ctx.quality = fitted;
                ctx.encoded_bytes = Some(encoded);
            }
        }
    }

    let encoded = ctx.encoded_bytes.ok_or_else(|| AppError::Pipeline {
        step: "output".into(),
        reason: "no encoded bytes produced".into(),
    })?;
    let output_size = encoded.len() as u64;

    if !config.dry_run {
        atomic_write(&task.output_path, &encoded).await?;
        #[cfg(feature = "review")]
        if config.burn_review_annotations {
            use crate::review::ReviewConversionBridge;
            if let Ok(service) = crate::review::ReviewService::open() {
                let _ = service.burn_annotations_for_export(
                    &task.input_path,
                    &task.output_path,
                    config.quality.value(),
                );
            }
        }
    }

    Ok(TaskOutcome { output_size })
}

/// 根据最大输入体积与任务数，估算更安全的并发上限。
///
/// 经验阈值：单文件 ≥ 32 MiB 时开始收紧；≥ 128 MiB 时进一步限制。
fn effective_concurrency(configured: usize, max_input_bytes: u64, task_count: usize) -> usize {
    let configured = configured.max(1);
    if task_count <= 1 {
        return 1.min(configured);
    }
    let by_size = if max_input_bytes >= 128 * 1024 * 1024 {
        1
    } else if max_input_bytes >= 32 * 1024 * 1024 {
        2
    } else {
        configured
    };
    configured.min(by_size).max(1)
}

#[cfg(test)]
mod concurrency_tests {
    use super::effective_concurrency;

    #[test]
    fn large_inputs_reduce_concurrency() {
        assert_eq!(effective_concurrency(8, 40 * 1024 * 1024, 10), 2);
        assert_eq!(effective_concurrency(8, 200 * 1024 * 1024, 10), 1);
        assert_eq!(effective_concurrency(8, 1024 * 1024, 10), 8);
        assert_eq!(effective_concurrency(8, 1024, 1), 1);
    }
}
