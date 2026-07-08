//! 批次对比结果。

use serde::{Deserialize, Serialize};

use super::evaluation::EvaluationStatus;
use super::imatest_module::ImatestModule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendStatus {
    Improved,
    Regressed,
    Unchanged,
    Incomparable,
}

impl TrendStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Improved => "改善",
            Self::Regressed => "退化",
            Self::Unchanged => "无变化",
            Self::Incomparable => "不可比较",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonRow {
    pub module: ImatestModule,
    pub metric_key: String,
    pub sample_name: Option<String>,
    pub baseline_value: Option<f64>,
    pub current_value: Option<f64>,
    pub delta: Option<f64>,
    pub delta_pct: Option<f64>,
    pub trend: TrendStatus,
    pub baseline_status: EvaluationStatus,
    pub current_status: EvaluationStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatchComparison {
    pub baseline_batch_id: String,
    pub baseline_batch_name: String,
    pub current_batch_id: String,
    pub current_batch_name: String,
    pub rows: Vec<ComparisonRow>,
}

impl BatchComparison {
    pub fn improved_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.trend == TrendStatus::Improved)
            .count()
    }

    pub fn regressed_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.trend == TrendStatus::Regressed)
            .count()
    }
}
