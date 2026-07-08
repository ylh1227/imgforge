//! 数据提取洞察：规则型总结、失败聚合和离群检测。

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::data_extract::domain::{EvaluationStatus, ExtractionBatch, ImatestModule, SummaryTable};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DataInsightReport {
    pub summary: Vec<String>,
    pub top_modules: Vec<InsightCount>,
    pub top_samples: Vec<InsightCount>,
    pub outliers: Vec<OutlierInsight>,
    pub unmapped_count: usize,
    pub low_ocr_confidence: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InsightCount {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutlierInsight {
    pub sample: String,
    pub metric: String,
    pub value: f64,
    pub lower_bound: f64,
    pub upper_bound: f64,
}

pub struct DataInsightService;

impl DataInsightService {
    pub fn analyze(batches: &[ExtractionBatch], table: &SummaryTable) -> DataInsightReport {
        let mut module_fail_warn: HashMap<ImatestModule, usize> = HashMap::new();
        let mut sample_fail_warn: HashMap<String, usize> = HashMap::new();
        let mut unmapped_count = 0usize;
        let mut low_ocr_confidence = 0usize;

        for batch in batches {
            unmapped_count += batch.unmapped_fields.len();
            for rec in &batch.records {
                if matches!(
                    rec.evaluation_status(),
                    EvaluationStatus::Fail | EvaluationStatus::Warn
                ) {
                    *module_fail_warn.entry(rec.module).or_default() += 1;
                    let sample = rec
                        .sample_name
                        .clone()
                        .or_else(|| {
                            rec.source_path
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                        })
                        .unwrap_or_else(|| "未命名样本".into());
                    *sample_fail_warn.entry(sample).or_default() += 1;
                }
                if rec
                    .ocr
                    .as_ref()
                    .and_then(|o| o.confidence)
                    .is_some_and(|c| c < 60.0)
                {
                    low_ocr_confidence += 1;
                }
            }
        }

        let top_modules = top_counts(module_fail_warn.into_iter().map(|(m, c)| InsightCount {
            label: m.short_label().into(),
            count: c,
        }));
        let top_samples = top_counts(
            sample_fail_warn
                .into_iter()
                .map(|(label, count)| InsightCount { label, count }),
        );
        let outliers = detect_outliers(table);

        let mut summary = Vec::new();
        if let Some(first) = top_modules.first() {
            summary.push(format!(
                "问题最多模块：{}（{} 条）",
                first.label, first.count
            ));
        }
        if let Some(first) = top_samples.first() {
            summary.push(format!(
                "问题最多样本：{}（{} 条）",
                first.label, first.count
            ));
        }
        if !outliers.is_empty() {
            summary.push(format!("检测到 {} 个数值离群点", outliers.len()));
        }
        if unmapped_count > 0 {
            summary.push(format!("存在 {} 个未映射字段", unmapped_count));
        }
        if low_ocr_confidence > 0 {
            summary.push(format!("存在 {} 条低置信度 OCR 记录", low_ocr_confidence));
        }
        if summary.is_empty() && !table.is_empty() {
            summary.push("未发现明显异常，所有导入记录可继续按状态筛选复核".into());
        }

        DataInsightReport {
            summary,
            top_modules,
            top_samples,
            outliers,
            unmapped_count,
            low_ocr_confidence,
        }
    }
}

fn top_counts(items: impl IntoIterator<Item = InsightCount>) -> Vec<InsightCount> {
    let mut items: Vec<_> = items.into_iter().collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    items.truncate(5);
    items
}

fn detect_outliers(table: &SummaryTable) -> Vec<OutlierInsight> {
    let mut by_metric: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    for row in &table.rows {
        for (metric, cell) in &row.values {
            if let Some(value) = cell.numeric {
                by_metric
                    .entry(metric.clone())
                    .or_default()
                    .push((row.sample_name.clone(), value));
            }
        }
    }

    let mut outliers = Vec::new();
    for (metric, values) in by_metric {
        if values.len() < 4 {
            continue;
        }
        let mut sorted: Vec<f64> = values.iter().map(|(_, v)| *v).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let q1 = percentile(&sorted, 0.25);
        let q3 = percentile(&sorted, 0.75);
        let iqr = q3 - q1;
        if iqr <= f64::EPSILON {
            continue;
        }
        let lower = q1 - 1.5 * iqr;
        let upper = q3 + 1.5 * iqr;
        for (sample, value) in values {
            if value < lower || value > upper {
                outliers.push(OutlierInsight {
                    sample,
                    metric: metric.clone(),
                    value,
                    lower_bound: lower,
                    upper_bound: upper,
                });
            }
        }
    }
    outliers
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_extract::domain::{
        ExtractionBatch, ExtractionRecord, ImatestModule, MetricValue,
    };
    use crate::data_extract::service::SummaryService;
    use std::path::PathBuf;

    #[test]
    fn detects_outlier_metric_values() {
        let mut batch = ExtractionBatch::new("b", PathBuf::from("/b"));
        for (sample, value) in [("a", 1.0), ("b", 1.1), ("c", 1.2), ("d", 9.9)] {
            batch.records.push(
                ExtractionRecord::new(
                    ImatestModule::Mtf,
                    "mtf50",
                    "MTF50",
                    MetricValue::number(value, ""),
                    PathBuf::from(format!("/{sample}.csv")),
                    "csv",
                )
                .with_sample(sample),
            );
        }
        let table = SummaryService::build(&[batch.clone()]);
        let report = DataInsightService::analyze(&[batch], &table);
        assert_eq!(report.outliers.len(), 1);
        assert_eq!(report.outliers[0].sample, "d");
    }
}
