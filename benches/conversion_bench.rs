//! 转换 / 缩放 / 扫描相关性能基准。
//!
//! 运行：`cargo bench --bench conversion_bench`

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use image::{ImageBuffer, Rgb};
use imgforge::core::types::{ImageFormat, Quality};
use imgforge::io::scanner::{scan_inputs, ScanFilter, ScanOptions};
use imgforge::processing::backends::native_backend::encode_dynamic_image;
use imgforge::processing::image_quality::resize_image;
use tempfile::TempDir;

fn sample_rgb(width: u32, height: u32) -> image::DynamicImage {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    });
    image::DynamicImage::ImageRgb8(img)
}

fn write_tree(root: &std::path::Path, count: usize) {
    for i in 0..count {
        let sub = root.join(format!("d{}", i % 4));
        std::fs::create_dir_all(&sub).unwrap();
        let path = sub.join(format!("img_{i}.png"));
        sample_rgb(64, 48).save(&path).unwrap();
    }
}

fn bench_encode_jpeg(c: &mut Criterion) {
    let img = sample_rgb(1920, 1080);
    let quality = Quality::DEFAULT;
    let mut group = c.benchmark_group("encode");
    group.throughput(Throughput::Elements(1));
    group.bench_function("jpeg_1080p_q85", |b| {
        b.iter(|| {
            let bytes = encode_dynamic_image(black_box(&img), ImageFormat::Jpeg, quality).unwrap();
            black_box(bytes.len())
        })
    });
    group.finish();
}

fn bench_resize_fit(c: &mut Criterion) {
    let img = sample_rgb(4000, 3000);
    let mut group = c.benchmark_group("resize");
    group.sample_size(20);
    group.bench_function("fit_4000x3000_to_1280", |b| {
        b.iter(|| {
            let out = resize_image(
                black_box(&img),
                Some(1280),
                Some(720),
                imgforge::core::types::ResizeMode::Fit,
            )
            .unwrap();
            black_box(out.width() + out.height())
        })
    });
    group.finish();
}

fn bench_scan_directory(c: &mut Criterion) {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("in");
    let output = tmp.path().join("out");
    std::fs::create_dir_all(&input).unwrap();
    write_tree(&input, 200);

    let options = ScanOptions {
        input_dir: input,
        output_dir: output,
        target_format: ImageFormat::WebP,
        recursive: true,
        preserve_structure: true,
        overwrite: true,
        bayer_only: false,
        filter: ScanFilter::default(),
        rename_template: None,
    };

    let mut group = c.benchmark_group("scan");
    group.throughput(Throughput::Elements(200));
    group.bench_function("scan_200_png", |b| {
        b.iter(|| {
            let tasks = scan_inputs(black_box(&options)).unwrap();
            black_box(tasks.len())
        })
    });
    group.finish();
}

fn bench_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(300))
        .measurement_time(Duration::from_secs(2))
}

criterion_group! {
    name = benches;
    config = bench_config();
    targets = bench_encode_jpeg, bench_resize_fit, bench_scan_directory
}
criterion_main!(benches);
