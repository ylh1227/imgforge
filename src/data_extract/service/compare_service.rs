//! 批次对比服务。

use crate::data_extract::domain::{
    BatchComparison, ComparisonRow, ExtractionBatch, ExtractionRecord, ImatestModule, TrendStatus,
};

pub struct CompareService;

impl CompareService {
    pub fn compare(baseline: &ExtractionBatch, current: &ExtractionBatch) -> BatchComparison {
        let mut rows = Vec::new();
        let baseline_map = index_records(&baseline.records);
        let current_map = index_records(&current.records);

        let keys: std::collections::BTreeSet<MatchKey> = baseline_map
            .keys()
            .chain(current_map.keys())
            .cloned()
            .collect();

        for key in keys {
            let b = baseline_map.get(&key);
            let c = current_map.get(&key);
            rows.push(build_row(key, b, c));
        }

        BatchComparison {
            baseline_batch_id: baseline.id.clone(),
            baseline_batch_name: baseline.name.clone(),
            current_batch_id: current.id.clone(),
            current_batch_name: current.name.clone(),
            rows,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MatchKey {
    module: ImatestModule,
    metric_key: String,
    sample: String,
}

fn index_records(
    records: &[ExtractionRecord],
) -> std::collections::BTreeMap<MatchKey, &ExtractionRecord> {
    let mut map = std::collections::BTreeMap::new();
    for rec in records {
        let sample = rec
            .sample_name
            .clone()
            .or_else(|| {
                rec.source_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .unwrap_or_default();
        let key = MatchKey {
            module: rec.module,
            metric_key: rec.metric_key.clone(),
            sample,
        };
        map.insert(key, rec);
    }
    map
}

fn build_row(
    key: MatchKey,
    baseline: Option<&&ExtractionRecord>,
    current: Option<&&ExtractionRecord>,
) -> ComparisonRow {
    let baseline_value = baseline.and_then(|r| r.value.numeric);
    let current_value = current.and_then(|r| r.value.numeric);
    let baseline_status = baseline.map(|r| r.evaluation_status()).unwrap_or_default();
    let current_status = current.map(|r| r.evaluation_status()).unwrap_or_default();

    let (delta, delta_pct, trend) = match (baseline_value, current_value) {
        (Some(b), Some(c)) => {
            let d = c - b;
            let pct = if b.abs() > f64::EPSILON {
                Some((d / b) * 100.0)
            } else {
                None
            };
            let trend = if d.abs() < f64::EPSILON {
                TrendStatus::Unchanged
            } else {
                infer_trend(key.module, key.metric_key.as_str(), d)
            };
            (Some(d), pct, trend)
        }
        _ => (None, None, TrendStatus::Incomparable),
    };

    ComparisonRow {
        module: key.module,
        metric_key: key.metric_key,
        sample_name: if key.sample.is_empty() {
            None
        } else {
            Some(key.sample)
        },
        baseline_value,
        current_value,
        delta,
        delta_pct,
        trend,
        baseline_status,
        current_status,
    }
}

fn infer_trend(module: ImatestModule, metric_key: &str, delta: f64) -> TrendStatus {
    let higher_is_better = matches!(
        (module, metric_key),
        (ImatestModule::Mtf, "mtf50")
            | (ImatestModule::Mtf, "mtf30")
            | (ImatestModule::DynamicRange, _)
            | (ImatestModule::Noise, "snr_db")
            | (ImatestModule::LowLight, "low_light_snr")
    );
    let lower_is_better = metric_key.contains("delta_e")
        || metric_key.contains("distortion")
        || metric_key.contains("error")
        || metric_key.contains("noise_pct")
        || metric_key.contains("shading");

    if higher_is_better {
        if delta > 0.0 {
            TrendStatus::Improved
        } else {
            TrendStatus::Regressed
        }
    } else if lower_is_better {
        if delta < 0.0 {
            TrendStatus::Improved
        } else {
            TrendStatus::Regressed
        }
    } else if delta.abs() < f64::EPSILON {
        TrendStatus::Unchanged
    } else {
        TrendStatus::Unchanged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_extract::domain::{ExtractionRecord, MetricValue};
    use std::path::PathBuf;

    fn rec(module: ImatestModule, key: &str, val: f64) -> ExtractionRecord {
        ExtractionRecord::new(
            module,
            key,
            key,
            MetricValue::number(val, ""),
            PathBuf::from("/t.csv"),
            "csv",
        )
    }

    #[test]
    fn compare_detects_regression() {
        let mut b = ExtractionBatch::new("b", PathBuf::from("/b"));
        b.records.push(rec(ImatestModule::Mtf, "mtf50", 0.45));
        let mut c = ExtractionBatch::new("c", PathBuf::from("/c"));
        c.records.push(rec(ImatestModule::Mtf, "mtf50", 0.30));
        let cmp = CompareService::compare(&b, &c);
        assert_eq!(cmp.rows[0].trend, TrendStatus::Regressed);
    }
}
