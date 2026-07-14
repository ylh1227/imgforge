//! 核心转换主链路集成测试：扫描 → 执行 → 原子写入 → 失败隔离。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use image::{ImageBuffer, Rgb};
use imgforge::config::AppConfig;
use imgforge::core::types::{Concurrency, ImageFormat, Quality};
use imgforge::io::atomic_write::{atomic_write, validate_output_path};
use imgforge::io::scanner::{scan_inputs, ScanFilter, ScanOptions};
use imgforge::job::run_batch;
use imgforge::scheduler::Executor;
use tempfile::TempDir;

fn write_sample_png(path: &Path, color: [u8; 3]) {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(32, 24, |_x, _y| Rgb(color));
    img.save(path).expect("write sample png");
}

fn base_config(input: PathBuf, output: PathBuf) -> AppConfig {
    AppConfig {
        input_dir: input,
        output_dir: output,
        target_format: ImageFormat::Jpeg,
        quality: Quality::DEFAULT,
        concurrency: Concurrency::new(2).expect("concurrency"),
        recursive: true,
        overwrite: true,
        preserve_structure: true,
        ..AppConfig::default()
    }
}

#[tokio::test]
async fn scan_and_convert_png_to_jpeg() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("in");
    let output = tmp.path().join("out");
    std::fs::create_dir_all(&input).unwrap();
    write_sample_png(&input.join("a.png"), [20, 120, 220]);
    write_sample_png(&input.join("b.png"), [200, 40, 40]);

    let config = base_config(input.clone(), output.clone());
    let report = run_batch(config, Arc::new(AtomicBool::new(false)), None)
        .await
        .expect("run_batch");

    assert_eq!(report.scanned, 2);
    assert_eq!(report.successes, 2);
    assert!(report.failures.is_empty());
    assert!(output.join("a.jpg").is_file());
    assert!(output.join("b.jpg").is_file());
    assert!(report.total_output_bytes > 0);
}

#[tokio::test]
async fn scan_inputs_builds_tasks_with_preserve_structure() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("photos");
    let nested = input.join("nested");
    let output = tmp.path().join("converted");
    std::fs::create_dir_all(&nested).unwrap();
    write_sample_png(&nested.join("shot.png"), [10, 10, 10]);

    let tasks = scan_inputs(&ScanOptions {
        input_dir: input,
        output_dir: output.clone(),
        target_format: ImageFormat::WebP,
        recursive: true,
        preserve_structure: true,
        overwrite: true,
        bayer_only: false,
        filter: ScanFilter::default(),
        rename_template: None,
    })
    .expect("scan");

    assert_eq!(tasks.len(), 1);
    assert_eq!(
        tasks[0].output_path,
        output.join("nested").join("shot.webp")
    );
}

#[tokio::test]
async fn executor_isolates_single_file_failure() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("in");
    let output = tmp.path().join("out");
    std::fs::create_dir_all(&input).unwrap();
    write_sample_png(&input.join("ok.png"), [1, 2, 3]);

    let bad = input.join("bad.png");
    std::fs::write(&bad, b"not-an-image").unwrap();

    let config = base_config(input, output.clone());
    let report = Executor::new(config, Arc::new(AtomicBool::new(false)))
        .run(
            scan_inputs(&ScanOptions {
                input_dir: tmp.path().join("in"),
                output_dir: output.clone(),
                target_format: ImageFormat::Jpeg,
                recursive: true,
                preserve_structure: true,
                overwrite: true,
                bayer_only: false,
                filter: ScanFilter::default(),
                rename_template: None,
            })
            .unwrap(),
            None,
        )
        .await
        .expect("executor")
        .report;

    assert_eq!(report.scanned, 2);
    assert_eq!(report.successes, 1);
    assert_eq!(report.failures.len(), 1);
    assert!(output.join("ok.jpg").is_file());
    assert!(!output.join("bad.jpg").exists());
}

#[tokio::test]
async fn atomic_write_replaces_destination() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("file.bin");
    std::fs::write(&path, b"old").unwrap();

    atomic_write(&path, b"new-content").await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"new-content");
    assert!(!tmp.path().join("file.bin.tmp").exists());
}

#[test]
fn validate_output_path_rejects_parent_dir_components() {
    let base = PathBuf::from("/tmp/out");
    let bad = PathBuf::from("/tmp/out/../escape.jpg");
    let err = validate_output_path(&base, &bad).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("path") || err.to_string().contains(".."));
}

#[cfg(feature = "incremental")]
#[tokio::test]
async fn incremental_skips_unchanged_files_on_second_run() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("in");
    let output = tmp.path().join("out");
    std::fs::create_dir_all(&input).unwrap();
    write_sample_png(&input.join("keep.png"), [90, 90, 90]);

    let mut config = base_config(input, output.clone());
    config.incremental = true;

    let first = run_batch(config.clone(), Arc::new(AtomicBool::new(false)), None)
        .await
        .unwrap();
    assert_eq!(first.successes, 1);
    assert!(output.join("keep.jpg").is_file());

    let second = run_batch(config, Arc::new(AtomicBool::new(false)), None)
        .await
        .unwrap();
    assert_eq!(second.scanned, 1);
    assert_eq!(second.skipped, 1);
    assert_eq!(second.successes, 0);
    assert_eq!(second.total, 0);
}
