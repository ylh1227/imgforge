//! CSV / JSON 报告导出。

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use super::insight_service::DataInsightReport;
use crate::data_extract::domain::{BatchComparison, ExtractionBatch, SourceKind, SummaryTable};
use crate::data_extract::error::DataExtractResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub row_count: usize,
    pub dest: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableExportColumn {
    pub key: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableExportSchema {
    pub columns: Vec<TableExportColumn>,
}

impl TableExportSchema {
    pub fn from_summary_table(table: &SummaryTable) -> Self {
        let mut columns = vec![
            TableExportColumn::enabled("batch", "batch"),
            TableExportColumn::enabled("sample", "sample"),
            TableExportColumn::enabled("source_path", "source_path"),
            TableExportColumn::enabled("source_kind", "source_kind"),
            TableExportColumn::enabled("status", "status"),
            TableExportColumn::enabled("warnings", "warnings"),
            TableExportColumn::enabled("conflicts", "conflicts"),
        ];
        for col in &table.columns {
            columns.push(TableExportColumn::enabled(&col.key, &col.label));
            columns.push(TableExportColumn::enabled(
                format!("{}_status", col.key),
                format!("{}_status", col.label),
            ));
        }
        Self { columns }
    }

    pub fn with_enabled_keys(mut self, keys: &[String]) -> Self {
        if keys.is_empty() {
            return self;
        }
        let enabled: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for column in &mut self.columns {
            column.enabled = enabled.contains(column.key.as_str());
        }
        self
    }

    pub fn enabled_keys(&self) -> Vec<&str> {
        self.columns
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.key.as_str())
            .collect()
    }
}

impl TableExportColumn {
    pub fn enabled(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            enabled: true,
        }
    }
}

pub struct DataExportService;

impl DataExportService {
    pub fn export_csv(batch: &ExtractionBatch, dest: &Path) -> DataExtractResult<ExportResult> {
        let mut file = File::create(dest)?;
        file.write_all(b"\xEF\xBB\xBF")?;
        let mut wtr = csv::Writer::from_writer(file);
        wtr.write_record([
            "module",
            "metric_key",
            "raw_name",
            "value",
            "unit",
            "numeric",
            "status",
            "threshold",
            "sample",
            "source_kind",
            "ocr_confidence",
            "source_path",
            "parser",
            "warnings",
        ])?;

        for rec in &batch.records {
            let warnings = rec
                .warnings
                .iter()
                .map(|w| w.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let numeric = rec
                .value
                .numeric
                .map(|n| format!("{n:.6}"))
                .unwrap_or_default();
            let unit = rec.value.unit.clone().unwrap_or_default();
            let sample = rec.sample_name.clone().unwrap_or_default();
            let source = rec.source_path.to_string_lossy().to_string();
            let status = rec.evaluation_status().label();
            let threshold = rec
                .evaluation
                .as_ref()
                .and_then(|e| e.rule_description.clone())
                .unwrap_or_default();
            let ocr_conf = rec
                .ocr
                .as_ref()
                .and_then(|o| o.confidence)
                .map(|c| format!("{c:.1}"))
                .unwrap_or_default();
            wtr.write_record([
                rec.module.label(),
                &rec.metric_key,
                &rec.raw_name,
                &rec.value.display_value(),
                &unit,
                &numeric,
                status,
                &threshold,
                &sample,
                rec.source_kind.label(),
                &ocr_conf,
                &source,
                &rec.parser_name,
                &warnings,
            ])?;
        }
        wtr.flush()?;
        Ok(ExportResult {
            row_count: batch.records.len(),
            dest: dest.to_path_buf(),
        })
    }

    pub fn export_json(batch: &ExtractionBatch, dest: &Path) -> DataExtractResult<ExportResult> {
        #[derive(Serialize)]
        struct Report<'a> {
            batch: &'a ExtractionBatch,
            summary: Summary<'a>,
            ocr_summary: OcrSummary,
        }
        #[derive(Serialize)]
        struct Summary<'a> {
            record_count: usize,
            modules: Vec<&'a str>,
            files_scanned: usize,
            files_parsed: usize,
            warning_count: usize,
            evaluation_summary: Option<&'a crate::data_extract::domain::EvaluationSummary>,
            unmapped_field_count: usize,
        }
        #[derive(Serialize)]
        struct OcrSummary {
            ocr_records: usize,
            low_confidence_records: usize,
        }

        let ocr_records = batch
            .records
            .iter()
            .filter(|r| r.source_kind == SourceKind::OcrImage)
            .count();
        let low_conf = batch
            .records
            .iter()
            .filter(|r| {
                r.ocr
                    .as_ref()
                    .and_then(|o| o.confidence)
                    .is_some_and(|c| c < 60.0)
            })
            .count();

        let report = Report {
            batch,
            summary: Summary {
                record_count: batch.record_count(),
                modules: batch.modules_found().iter().map(|m| m.label()).collect(),
                files_scanned: batch.files_scanned,
                files_parsed: batch.files_parsed,
                warning_count: batch.warnings.len(),
                evaluation_summary: batch.evaluation_summary.as_ref(),
                unmapped_field_count: batch.unmapped_fields.len(),
            },
            ocr_summary: OcrSummary {
                ocr_records,
                low_confidence_records: low_conf,
            },
        };

        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(dest, json)?;
        Ok(ExportResult {
            row_count: batch.records.len(),
            dest: dest.to_path_buf(),
        })
    }

    pub fn export_comparison(
        cmp: &BatchComparison,
        dest: &Path,
    ) -> DataExtractResult<ExportResult> {
        let mut file = File::create(dest)?;
        file.write_all(b"\xEF\xBB\xBF")?;
        let mut wtr = csv::Writer::from_writer(file);
        wtr.write_record([
            "module",
            "metric_key",
            "sample",
            "baseline_value",
            "current_value",
            "delta",
            "delta_pct",
            "trend",
            "baseline_status",
            "current_status",
        ])?;
        for row in &cmp.rows {
            wtr.write_record([
                row.module.label(),
                &row.metric_key,
                row.sample_name.as_deref().unwrap_or(""),
                &fmt_opt(row.baseline_value),
                &fmt_opt(row.current_value),
                &fmt_opt(row.delta),
                &row.delta_pct.map(|p| format!("{p:.2}")).unwrap_or_default(),
                row.trend.label(),
                row.baseline_status.label(),
                row.current_status.label(),
            ])?;
        }
        wtr.flush()?;
        Ok(ExportResult {
            row_count: cmp.rows.len(),
            dest: dest.to_path_buf(),
        })
    }

    pub fn export_summary_csv(
        table: &SummaryTable,
        dest: &Path,
    ) -> DataExtractResult<ExportResult> {
        let schema = TableExportSchema::from_summary_table(table);
        Self::export_summary_csv_with_schema(table, &schema, dest)
    }

    pub fn export_summary_csv_with_schema(
        table: &SummaryTable,
        schema: &TableExportSchema,
        dest: &Path,
    ) -> DataExtractResult<ExportResult> {
        let mut file = File::create(dest)?;
        file.write_all(b"\xEF\xBB\xBF")?;
        let mut wtr = csv::Writer::from_writer(file);

        let keys = schema.enabled_keys();
        wtr.write_record(keys.iter().copied())?;

        for row in &table.rows {
            let record: Vec<String> = keys
                .iter()
                .map(|key| summary_cell_value(table, row, key))
                .collect();
            wtr.write_record(record)?;
        }

        wtr.flush()?;
        Ok(ExportResult {
            row_count: table.rows.len(),
            dest: dest.to_path_buf(),
        })
    }

    pub fn export_summary_json(
        table: &SummaryTable,
        dest: &Path,
    ) -> DataExtractResult<ExportResult> {
        let schema = TableExportSchema::from_summary_table(table);
        Self::export_summary_json_with_schema(table, &schema, dest)
    }

    pub fn export_summary_json_with_schema(
        table: &SummaryTable,
        schema: &TableExportSchema,
        dest: &Path,
    ) -> DataExtractResult<ExportResult> {
        Self::export_summary_json_with_insights(table, schema, None, dest)
    }

    pub fn export_summary_json_with_insights(
        table: &SummaryTable,
        schema: &TableExportSchema,
        insights: Option<&DataInsightReport>,
        dest: &Path,
    ) -> DataExtractResult<ExportResult> {
        #[derive(Serialize)]
        struct SummaryReport<'a> {
            table: &'a SummaryTable,
            schema: &'a TableExportSchema,
            #[serde(skip_serializing_if = "Option::is_none")]
            insights: Option<&'a DataInsightReport>,
            row_count: usize,
            column_count: usize,
        }

        let report = SummaryReport {
            table,
            schema,
            insights,
            row_count: table.rows.len(),
            column_count: table.columns.len(),
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(dest, json)?;
        Ok(ExportResult {
            row_count: table.rows.len(),
            dest: dest.to_path_buf(),
        })
    }

    pub fn export_summary_html_report(
        table: &SummaryTable,
        schema: &TableExportSchema,
        insights: &DataInsightReport,
        dest: &Path,
    ) -> DataExtractResult<ExportResult> {
        let mut html = String::new();
        html.push_str(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>ImgForge Report</title>",
        );
        html.push_str("<style>body{font-family:-apple-system,BlinkMacSystemFont,sans-serif;margin:24px}table{border-collapse:collapse;width:100%}td,th{border:1px solid #ddd;padding:6px;font-size:12px}th{background:#f4f4f4}.muted{color:#666}</style>");
        html.push_str("</head><body><h1>ImgForge 数据提取报告</h1>");
        html.push_str("<h2>洞察摘要</h2><ul>");
        for item in &insights.summary {
            html.push_str(&format!("<li>{}</li>", escape_html(item)));
        }
        html.push_str("</ul>");
        if !insights.outliers.is_empty() {
            html.push_str("<h3>离群点</h3><ul>");
            for outlier in insights.outliers.iter().take(20) {
                html.push_str(&format!(
                    "<li>{} / {} = {:.4}</li>",
                    escape_html(&outlier.sample),
                    escape_html(&outlier.metric),
                    outlier.value
                ));
            }
            html.push_str("</ul>");
        }
        html.push_str("<h2>汇总表</h2><table><thead><tr>");
        let keys = schema.enabled_keys();
        for key in &keys {
            html.push_str(&format!("<th>{}</th>", escape_html(key)));
        }
        html.push_str("</tr></thead><tbody>");
        for row in &table.rows {
            html.push_str("<tr>");
            for key in &keys {
                html.push_str(&format!(
                    "<td>{}</td>",
                    escape_html(&summary_cell_value(table, row, key))
                ));
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table><p class=\"muted\">Generated by ImgForge</p></body></html>");
        std::fs::write(dest, html)?;
        Ok(ExportResult {
            row_count: table.rows.len(),
            dest: dest.to_path_buf(),
        })
    }
}

fn summary_cell_value(
    table: &SummaryTable,
    row: &crate::data_extract::domain::SummaryRow,
    key: &str,
) -> String {
    match key {
        "batch" => row.batch_name.clone(),
        "sample" => row.sample_name.clone(),
        "source_path" => row.source_path.to_string_lossy().to_string(),
        "source_kind" => row.source_kind.label().to_string(),
        "status" => row.status.label().to_string(),
        "warnings" => row.warning_count.to_string(),
        "conflicts" => row.conflict_count.to_string(),
        _ => {
            if let Some(metric_key) = key.strip_suffix("_status") {
                if table.columns.iter().any(|c| c.key == metric_key) {
                    return row
                        .values
                        .get(metric_key)
                        .map(|cell| cell.status.label().to_string())
                        .unwrap_or_default();
                }
            }
            row.values
                .get(key)
                .map(|cell| cell.display.clone())
                .unwrap_or_default()
        }
    }
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|n| format!("{n:.6}")).unwrap_or_default()
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_extract::domain::{
        ExtractionBatch, ExtractionRecord, ImatestModule, MetricValue,
    };
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn sample_batch() -> ExtractionBatch {
        let mut batch = ExtractionBatch::new("test", PathBuf::from("/tmp"));
        batch.records.push(ExtractionRecord::new(
            ImatestModule::Mtf,
            "mtf50",
            "MTF50",
            MetricValue::number(0.42, "lp/ph"),
            PathBuf::from("/tmp/mtf.csv"),
            "csv",
        ));
        batch
    }

    #[test]
    fn export_csv_json() {
        let batch = sample_batch();
        let csv = NamedTempFile::new().unwrap();
        let json = NamedTempFile::new().unwrap();
        let r1 = DataExportService::export_csv(&batch, csv.path()).unwrap();
        let r2 = DataExportService::export_json(&batch, json.path()).unwrap();
        assert_eq!(r1.row_count, 1);
        assert_eq!(r2.row_count, 1);
    }

    #[test]
    fn summary_export_schema_controls_columns() {
        let batch = sample_batch();
        let table = crate::data_extract::service::SummaryService::build(&[batch]);
        let mut schema = TableExportSchema::from_summary_table(&table);
        for col in &mut schema.columns {
            col.enabled = matches!(col.key.as_str(), "batch" | "MTF.mtf50");
        }
        let csv = NamedTempFile::new().unwrap();
        let result =
            DataExportService::export_summary_csv_with_schema(&table, &schema, csv.path()).unwrap();
        let raw = std::fs::read_to_string(csv.path()).unwrap();
        assert_eq!(result.row_count, 1);
        assert!(raw.contains("batch,MTF.mtf50"));
        assert!(!raw.contains("source_path"));
    }
}
