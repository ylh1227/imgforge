//! 阈值评价状态与结果。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EvaluationStatus {
    #[default]
    Unknown,
    Pass,
    Warn,
    Fail,
}

impl EvaluationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "未判定",
            Self::Pass => "通过",
            Self::Warn => "警告",
            Self::Fail => "失败",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EvaluationResult {
    pub status: EvaluationStatus,
    pub rule_description: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationSummary {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub unknown: usize,
}

impl EvaluationSummary {
    pub fn from_statuses(statuses: impl IntoIterator<Item = EvaluationStatus>) -> Self {
        let mut s = Self::default();
        for st in statuses {
            match st {
                EvaluationStatus::Pass => s.pass += 1,
                EvaluationStatus::Warn => s.warn += 1,
                EvaluationStatus::Fail => s.fail += 1,
                EvaluationStatus::Unknown => s.unknown += 1,
            }
        }
        s
    }
}
