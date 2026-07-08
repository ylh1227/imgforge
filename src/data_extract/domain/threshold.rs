//! 阈值规则与配置。

use serde::{Deserialize, Serialize};

use super::evaluation::{EvaluationResult, EvaluationStatus};
use super::extraction_record::ExtractionRecord;
use super::imatest_module::ImatestModule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdOp {
    Gte,
    Lte,
    AbsLte,
    Between,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThresholdRule {
    pub module: ImatestModule,
    pub metric_key: String,
    pub operator: ThresholdOp,
    pub warn_limit: f64,
    pub fail_limit: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ThresholdProfile {
    pub rules: Vec<ThresholdRule>,
}

impl ThresholdProfile {
    pub fn default_rules() -> Self {
        Self {
            rules: vec![
                ThresholdRule {
                    module: ImatestModule::Mtf,
                    metric_key: "mtf50".into(),
                    operator: ThresholdOp::Gte,
                    warn_limit: 0.35,
                    fail_limit: 0.30,
                    unit: String::new(),
                    description: "MTF50 越高越好".into(),
                },
                ThresholdRule {
                    module: ImatestModule::ColorAccuracy,
                    metric_key: "delta_e_mean".into(),
                    operator: ThresholdOp::Lte,
                    warn_limit: 3.0,
                    fail_limit: 5.0,
                    unit: String::new(),
                    description: "平均 Delta E 越低越好".into(),
                },
                ThresholdRule {
                    module: ImatestModule::Noise,
                    metric_key: "snr_db".into(),
                    operator: ThresholdOp::Gte,
                    warn_limit: 32.0,
                    fail_limit: 28.0,
                    unit: "dB".into(),
                    description: "SNR 越高越好".into(),
                },
                ThresholdRule {
                    module: ImatestModule::Distortion,
                    metric_key: "tv_distortion_pct".into(),
                    operator: ThresholdOp::AbsLte,
                    warn_limit: 2.0,
                    fail_limit: 3.0,
                    unit: "%".into(),
                    description: "TV 畸变绝对值越小越好".into(),
                },
                ThresholdRule {
                    module: ImatestModule::ExposureError,
                    metric_key: "exposure_error_ev".into(),
                    operator: ThresholdOp::AbsLte,
                    warn_limit: 0.3,
                    fail_limit: 0.5,
                    unit: "EV".into(),
                    description: "曝光误差绝对值越小越好".into(),
                },
            ],
        }
    }

    pub fn evaluate(&self, record: &ExtractionRecord) -> EvaluationResult {
        let Some(value) = record.value.numeric else {
            return EvaluationResult::default();
        };

        let Some(rule) = self
            .rules
            .iter()
            .find(|r| r.module == record.module && r.metric_key == record.metric_key)
        else {
            return EvaluationResult::default();
        };

        let (status, reason) = match rule.operator {
            ThresholdOp::Gte => {
                if value >= rule.warn_limit {
                    (
                        EvaluationStatus::Pass,
                        format!("{value:.4} >= {}", rule.warn_limit),
                    )
                } else if value >= rule.fail_limit {
                    (
                        EvaluationStatus::Warn,
                        format!("{value:.4} 介于失败线与警告线"),
                    )
                } else {
                    (
                        EvaluationStatus::Fail,
                        format!("{value:.4} < {}", rule.fail_limit),
                    )
                }
            }
            ThresholdOp::Lte => {
                if value <= rule.warn_limit {
                    (
                        EvaluationStatus::Pass,
                        format!("{value:.4} <= {}", rule.warn_limit),
                    )
                } else if value <= rule.fail_limit {
                    (
                        EvaluationStatus::Warn,
                        format!("{value:.4} 介于警告线与失败线"),
                    )
                } else {
                    (
                        EvaluationStatus::Fail,
                        format!("{value:.4} > {}", rule.fail_limit),
                    )
                }
            }
            ThresholdOp::AbsLte => {
                let abs = value.abs();
                if abs <= rule.warn_limit {
                    (
                        EvaluationStatus::Pass,
                        format!("|{value:.4}| <= {}", rule.warn_limit),
                    )
                } else if abs <= rule.fail_limit {
                    (
                        EvaluationStatus::Warn,
                        format!("|{value:.4}| 介于警告线与失败线"),
                    )
                } else {
                    (
                        EvaluationStatus::Fail,
                        format!("|{value:.4}| > {}", rule.fail_limit),
                    )
                }
            }
            ThresholdOp::Between => {
                let lo = rule.fail_limit.min(rule.warn_limit);
                let hi = rule.fail_limit.max(rule.warn_limit);
                if (lo..=hi).contains(&value) {
                    (
                        EvaluationStatus::Pass,
                        format!("{value:.4} 在 [{lo}, {hi}] 内"),
                    )
                } else {
                    (
                        EvaluationStatus::Fail,
                        format!("{value:.4} 超出 [{lo}, {hi}]"),
                    )
                }
            }
        };

        EvaluationResult {
            status,
            rule_description: Some(rule.description.clone()),
            reason: Some(reason),
        }
    }
}
