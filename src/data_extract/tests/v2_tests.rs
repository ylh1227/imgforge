//! Imatest v2：阈值、对比、OCR 文本解析测试。

use std::path::PathBuf;

use crate::data_extract::domain::OcrMetadata;
use crate::data_extract::domain::{
    EvaluationStatus, ExtractionBatch, ExtractionRecord, ImatestModule, MetricValue,
    ThresholdProfile,
};
use crate::data_extract::parser::ocr_text_parser::parse_ocr_text;
use crate::data_extract::service::{
    CompareService, DataExportService, DataExtractService, SummaryService, ThresholdService,
};

fn mtf_record(val: f64) -> ExtractionRecord {
    ExtractionRecord::new(
        ImatestModule::Mtf,
        "mtf50",
        "MTF50",
        MetricValue::number(val, "lp/ph"),
        PathBuf::from("/t/mtf.csv"),
        "csv",
    )
}

fn rec(module: ImatestModule, key: &str, val: f64, sample: &str, file: &str) -> ExtractionRecord {
    ExtractionRecord::new(
        module,
        key,
        key,
        MetricValue::number(val, ""),
        PathBuf::from(file),
        "csv",
    )
    .with_sample(sample)
}

#[test]
fn threshold_mtf50_pass_warn_fail() {
    let profile = ThresholdProfile::default_rules();
    let mut pass = mtf_record(0.40);
    let mut warn = mtf_record(0.32);
    let mut fail = mtf_record(0.25);

    pass.evaluation = Some(profile.evaluate(&pass));
    warn.evaluation = Some(profile.evaluate(&warn));
    fail.evaluation = Some(profile.evaluate(&fail));

    assert_eq!(pass.evaluation_status(), EvaluationStatus::Pass);
    assert_eq!(warn.evaluation_status(), EvaluationStatus::Warn);
    assert_eq!(fail.evaluation_status(), EvaluationStatus::Fail);
}

#[test]
fn batch_apply_thresholds_updates_summary() {
    let mut batch = ExtractionBatch::new("t", PathBuf::from("/t"));
    batch.records.push(mtf_record(0.40));
    batch.records.push(mtf_record(0.20));
    let profile = ThresholdProfile::default_rules();
    batch.apply_thresholds(&profile);
    let summary = batch.evaluation_summary.unwrap();
    assert_eq!(summary.pass, 1);
    assert_eq!(summary.fail, 1);
}

#[test]
fn compare_service_delta_and_trend() {
    let mut baseline = ExtractionBatch::new("b", PathBuf::from("/b"));
    baseline.records.push(mtf_record(0.45));
    let mut current = ExtractionBatch::new("c", PathBuf::from("/c"));
    current.records.push(mtf_record(0.50));
    let cmp = CompareService::compare(&baseline, &current);
    assert_eq!(cmp.rows.len(), 1);
    assert!(cmp.rows[0].delta.unwrap() > 0.0);
    assert_eq!(
        cmp.rows[0].trend,
        crate::data_extract::domain::TrendStatus::Improved
    );
}

#[test]
fn threshold_service_default_load() {
    let p = ThresholdService::load_or_default();
    assert!(!p.rules.is_empty());
}

#[test]
fn ocr_text_parser_extracts_mtf_from_plaintext() {
    let text = "Imatest Results\nROI: Center\nMTF50: 0.38 lp/ph\nMTF30: 0.25";
    let meta = OcrMetadata::new("test", "eng");
    let out = parse_ocr_text(PathBuf::from("/t/shot.png").as_path(), text, meta).unwrap();
    assert!(!out.records.is_empty());
    assert!(out.records.iter().any(|r| r.metric_key == "mtf50"
        && r.source_kind == crate::data_extract::domain::SourceKind::OcrImage));
}

#[test]
fn extract_with_thresholds_from_csv_dir() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mtf.csv");
    std::fs::write(&path, "ROI,MTF50\nCenter,0.42\n").unwrap();
    let profile = ThresholdProfile::default_rules();
    let batch = DataExtractService::extract_with_thresholds(dir.path(), Some(&profile)).unwrap();
    assert_eq!(batch.files_parsed, 1);
    assert!(batch.evaluation_summary.is_some());
}

#[test]
fn summary_service_builds_wide_table_across_batches() {
    let mut b1 = ExtractionBatch::new("batch-a", PathBuf::from("/a"));
    b1.records.push(rec(
        ImatestModule::Mtf,
        "mtf50",
        0.42,
        "center",
        "/a/sample.csv",
    ));
    b1.records.push(rec(
        ImatestModule::ColorAccuracy,
        "delta_e_mean",
        2.1,
        "center",
        "/a/sample.csv",
    ));
    let mut b2 = ExtractionBatch::new("batch-b", PathBuf::from("/b"));
    b2.records.push(rec(
        ImatestModule::Mtf,
        "mtf50",
        0.40,
        "center",
        "/b/sample.csv",
    ));

    let table = SummaryService::build(&[b1, b2]);
    assert_eq!(table.rows.len(), 1);
    assert!(table.columns.iter().any(|c| c.key == "MTF.mtf50"));
    assert!(table
        .columns
        .iter()
        .any(|c| c.key == "色彩还原.delta_e_mean"));
    assert_eq!(
        table.rows[0]
            .values
            .get("MTF.mtf50")
            .unwrap()
            .record_ref
            .batch_index,
        0
    );
    assert_eq!(table.rows[0].conflict_count, 1);
}

#[test]
fn summary_service_tracks_conflicts_and_row_status() {
    let mut batch = ExtractionBatch::new("batch", PathBuf::from("/b"));
    batch.records.push(mtf_record(0.40).with_sample("center"));
    batch.records.push(mtf_record(0.20).with_sample("center"));
    batch.apply_thresholds(&ThresholdProfile::default_rules());

    let table = SummaryService::build(&[batch]);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].conflict_count, 1);
    assert_eq!(table.rows[0].status, EvaluationStatus::Fail);
}

#[test]
fn summary_service_builds_cross_batch_detail_table() {
    let mut b1 = ExtractionBatch::new("batch-a", PathBuf::from("/a"));
    b1.records.push(rec(
        ImatestModule::Mtf,
        "mtf50",
        0.42,
        "center",
        "/a/sample.csv",
    ));
    let mut b2 = ExtractionBatch::new("batch-b", PathBuf::from("/b"));
    b2.records.push(rec(
        ImatestModule::ColorAccuracy,
        "delta_e_mean",
        2.1,
        "center",
        "/b/sample.csv",
    ));

    let table = SummaryService::detail_table(&[b1, b2]);
    assert_eq!(table.rows.len(), 2);
    assert!(table.columns.iter().any(|c| c.key == "module"));
    assert_eq!(
        table.rows[1].values.get("metric_key").unwrap().display,
        "delta_e_mean"
    );
}

#[test]
fn export_summary_csv_json() {
    let mut batch = ExtractionBatch::new("batch", PathBuf::from("/b"));
    batch.records.push(mtf_record(0.40).with_sample("center"));
    let table = SummaryService::build(&[batch]);
    let csv = tempfile::NamedTempFile::new().unwrap();
    let json = tempfile::NamedTempFile::new().unwrap();

    let csv_result = DataExportService::export_summary_csv(&table, csv.path()).unwrap();
    let json_result = DataExportService::export_summary_json(&table, json.path()).unwrap();

    assert_eq!(csv_result.row_count, 1);
    assert_eq!(json_result.row_count, 1);
}
