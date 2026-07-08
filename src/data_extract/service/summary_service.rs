//! 汇总表构建服务。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::data_extract::domain::{
    EvaluationStatus, ExtractionBatch, ExtractionRecord, ImatestModule, MetricValue, SourceKind,
    SummaryCell, SummaryColumn, SummaryRecordRef, SummaryRow, SummaryTable,
};

pub struct SummaryService;

impl SummaryService {
    pub fn build(batches: &[ExtractionBatch]) -> SummaryTable {
        let mut rows: BTreeMap<RowKey, SummaryRow> = BTreeMap::new();
        let mut columns: BTreeMap<ColumnSortKey, SummaryColumn> = BTreeMap::new();

        for (batch_index, batch) in batches.iter().enumerate() {
            for (record_index, rec) in batch.records.iter().enumerate() {
                let col_key = summary_column_key(rec);
                columns
                    .entry(ColumnSortKey::from_record(rec))
                    .or_insert_with(|| SummaryColumn {
                        key: col_key.clone(),
                        label: col_key.clone(),
                    });
                let key = RowKey::from_record(batch_index, batch, rec);
                let row = rows.entry(key.clone()).or_insert_with(|| SummaryRow {
                    batch_index,
                    batch_id: batch.id.clone(),
                    batch_name: batch.name.clone(),
                    sample_name: key.sample_name.clone(),
                    source_path: source_path(batch, rec),
                    source_kind: rec.source_kind,
                    status: EvaluationStatus::Pass,
                    warning_count: 0,
                    conflict_count: 0,
                    values: BTreeMap::new(),
                });

                row.warning_count += rec.warnings.len();
                row.status = merge_status(row.status, rec.evaluation_status());
                row.source_kind = merge_source_kind(row.source_kind, rec.source_kind);

                let cell = SummaryCell {
                    display: rec.value.display_value(),
                    numeric: rec.value.numeric,
                    status: rec.evaluation_status(),
                    record_ref: SummaryRecordRef {
                        batch_index,
                        record_index,
                    },
                };

                match row.values.get(&col_key) {
                    Some(existing) => {
                        row.conflict_count += 1;
                        if should_replace_cell(existing, &cell) {
                            row.values.insert(col_key, cell);
                        }
                    }
                    None => {
                        row.values.insert(col_key, cell);
                    }
                }
            }
        }

        SummaryTable {
            columns: columns.into_values().collect(),
            rows: rows.into_values().collect(),
        }
    }

    pub fn detail_table(batches: &[ExtractionBatch]) -> SummaryTable {
        let columns = vec![
            SummaryColumn::new("module", "module"),
            SummaryColumn::new("metric_key", "metric_key"),
            SummaryColumn::new("raw_name", "raw_name"),
            SummaryColumn::new("value", "value"),
            SummaryColumn::new("unit", "unit"),
            SummaryColumn::new("numeric", "numeric"),
            SummaryColumn::new("threshold", "threshold"),
            SummaryColumn::new("parser", "parser"),
            SummaryColumn::new("ocr_confidence", "ocr_confidence"),
        ];
        let mut rows = Vec::new();

        for (batch_index, batch) in batches.iter().enumerate() {
            for (record_index, rec) in batch.records.iter().enumerate() {
                let sample_name = sample_name(batch, rec);
                let source_path = source_path(batch, rec);
                let status = rec.evaluation_status();
                let record_ref = SummaryRecordRef {
                    batch_index,
                    record_index,
                };
                let mut values = BTreeMap::new();
                insert_detail_cell(
                    &mut values,
                    "module",
                    rec.module.label(),
                    status,
                    record_ref,
                );
                insert_detail_cell(
                    &mut values,
                    "metric_key",
                    &rec.metric_key,
                    status,
                    record_ref,
                );
                insert_detail_cell(&mut values, "raw_name", &rec.raw_name, status, record_ref);
                insert_detail_cell(
                    &mut values,
                    "value",
                    rec.value.display_value(),
                    status,
                    record_ref,
                );
                insert_detail_cell(
                    &mut values,
                    "unit",
                    rec.value.unit.clone().unwrap_or_default(),
                    status,
                    record_ref,
                );
                insert_detail_cell(
                    &mut values,
                    "numeric",
                    fmt_numeric(&rec.value),
                    status,
                    record_ref,
                );
                insert_detail_cell(
                    &mut values,
                    "threshold",
                    rec.evaluation
                        .as_ref()
                        .and_then(|e| e.rule_description.clone())
                        .unwrap_or_default(),
                    status,
                    record_ref,
                );
                insert_detail_cell(&mut values, "parser", &rec.parser_name, status, record_ref);
                insert_detail_cell(
                    &mut values,
                    "ocr_confidence",
                    rec.ocr
                        .as_ref()
                        .and_then(|o| o.confidence)
                        .map(|c| format!("{c:.1}"))
                        .unwrap_or_default(),
                    status,
                    record_ref,
                );
                rows.push(SummaryRow {
                    batch_index,
                    batch_id: batch.id.clone(),
                    batch_name: batch.name.clone(),
                    sample_name,
                    source_path,
                    source_kind: rec.source_kind,
                    status,
                    warning_count: rec.warnings.len(),
                    conflict_count: 0,
                    values,
                });
            }
        }

        SummaryTable { columns, rows }
    }
}

impl SummaryColumn {
    fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RowKey {
    sample_name: String,
    source_name: String,
}

impl RowKey {
    fn from_record(_batch_index: usize, batch: &ExtractionBatch, rec: &ExtractionRecord) -> Self {
        let source_path = source_path(batch, rec);
        Self {
            sample_name: sample_name(batch, rec),
            source_name: source_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| source_path.to_string_lossy().to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ColumnSortKey {
    module_index: usize,
    metric_key: String,
    key: String,
}

impl ColumnSortKey {
    fn from_record(rec: &ExtractionRecord) -> Self {
        let key = summary_column_key(rec);
        Self {
            module_index: ImatestModule::ALL
                .iter()
                .position(|m| *m == rec.module)
                .unwrap_or(usize::MAX),
            metric_key: rec.metric_key.clone(),
            key,
        }
    }
}

fn summary_column_key(rec: &ExtractionRecord) -> String {
    format!("{}.{}", rec.module.short_label(), rec.metric_key)
}

fn sample_name(batch: &ExtractionBatch, rec: &ExtractionRecord) -> String {
    rec.sample_name
        .clone()
        .or_else(|| file_stem(&rec.source_path))
        .or_else(|| file_stem(&batch.source_root))
        .unwrap_or_else(|| "—".into())
}

fn source_path(batch: &ExtractionBatch, rec: &ExtractionRecord) -> PathBuf {
    if rec.source_path.as_os_str().is_empty() {
        batch.source_root.clone()
    } else {
        rec.source_path.clone()
    }
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().to_string())
}

fn should_replace_cell(existing: &SummaryCell, new_cell: &SummaryCell) -> bool {
    new_cell.record_ref.batch_index < existing.record_ref.batch_index
        || (new_cell.record_ref.batch_index == existing.record_ref.batch_index
            && existing.numeric.is_none()
            && new_cell.numeric.is_some())
}

fn insert_detail_cell(
    values: &mut BTreeMap<String, SummaryCell>,
    key: &str,
    display: impl Into<String>,
    status: EvaluationStatus,
    record_ref: SummaryRecordRef,
) {
    values.insert(
        key.to_string(),
        SummaryCell {
            display: display.into(),
            numeric: None,
            status,
            record_ref,
        },
    );
}

fn fmt_numeric(value: &MetricValue) -> String {
    value.numeric.map(|n| format!("{n:.6}")).unwrap_or_default()
}

fn merge_status(left: EvaluationStatus, right: EvaluationStatus) -> EvaluationStatus {
    use EvaluationStatus::*;
    match (left, right) {
        (Fail, _) | (_, Fail) => Fail,
        (Warn, _) | (_, Warn) => Warn,
        (Unknown, _) | (_, Unknown) => Unknown,
        (Pass, Pass) => Pass,
    }
}

fn merge_source_kind(left: SourceKind, right: SourceKind) -> SourceKind {
    if left == right {
        left
    } else if matches!(left, SourceKind::OcrImage) || matches!(right, SourceKind::OcrImage) {
        SourceKind::OcrImage
    } else if matches!(left, SourceKind::HtmlFile) || matches!(right, SourceKind::HtmlFile) {
        SourceKind::HtmlFile
    } else if matches!(left, SourceKind::TextFile) || matches!(right, SourceKind::TextFile) {
        SourceKind::TextFile
    } else {
        SourceKind::StructuredFile
    }
}
