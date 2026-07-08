//! 指标值类型。

use serde::{Deserialize, Serialize};

/// 通过/失败状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PassStatus {
    Unknown,
    Pass,
    Fail,
}

impl PassStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "—",
            Self::Pass => "通过",
            Self::Fail => "未通过",
        }
    }
}

/// 单条指标值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricValue {
    pub numeric: Option<f64>,
    pub text: Option<String>,
    pub unit: Option<String>,
    pub pass_status: PassStatus,
}

impl MetricValue {
    pub fn number(value: f64, unit: impl Into<String>) -> Self {
        Self {
            numeric: Some(value),
            text: None,
            unit: Some(unit.into()),
            pass_status: PassStatus::Unknown,
        }
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self {
            numeric: None,
            text: Some(value.into()),
            unit: None,
            pass_status: PassStatus::Unknown,
        }
    }

    pub fn display_value(&self) -> String {
        if let Some(n) = self.numeric {
            if let Some(ref u) = self.unit {
                if u.is_empty() {
                    format!("{n:.4}")
                } else {
                    format!("{n:.4} {u}")
                }
            } else {
                format!("{n:.4}")
            }
        } else {
            self.text.clone().unwrap_or_default()
        }
    }
}
