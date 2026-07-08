//! 数据提取结构化查询与关键词自然语言映射。

use crate::data_extract::domain::{EvaluationStatus, ImatestModule, SummaryRow};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataQuery {
    pub module: Option<ImatestModule>,
    pub status: Option<EvaluationStatus>,
    pub metric: Option<String>,
    pub text: Option<String>,
    pub outlier_only: bool,
}

pub struct DataQueryService;

impl DataQueryService {
    pub fn parse(input: &str) -> DataQuery {
        let mut query = DataQuery::default();
        let input = input.trim();
        if input.is_empty() {
            return query;
        }
        let lower = input.to_ascii_lowercase();
        if lower.contains("失败") || lower.contains("fail") {
            query.status = Some(EvaluationStatus::Fail);
        } else if lower.contains("警告") || lower.contains("warn") {
            query.status = Some(EvaluationStatus::Warn);
        }
        if lower.contains("离群") || lower.contains("异常") || lower.contains("outlier") {
            query.outlier_only = true;
        }
        for token in input.split_whitespace() {
            if let Some(value) = token.strip_prefix("module:") {
                query.module = module_from_text(value);
            } else if let Some(value) = token.strip_prefix("status:") {
                query.status = status_from_text(value);
            } else if let Some(value) = token.strip_prefix("metric:") {
                query.metric = Some(value.to_ascii_lowercase());
            } else if let Some(value) = token.strip_prefix("text:") {
                query.text = Some(value.to_ascii_lowercase());
            }
        }
        if query.module.is_none() {
            query.module = module_from_text(input);
        }
        if query.text.is_none()
            && query.module.is_none()
            && query.status.is_none()
            && query.metric.is_none()
            && !query.outlier_only
        {
            query.text = Some(lower);
        }
        query
    }

    pub fn matches_row(query: &DataQuery, row: &SummaryRow, outlier_metrics: &[String]) -> bool {
        if let Some(status) = query.status {
            if row.status != status {
                return false;
            }
        }
        if query.outlier_only
            && !row
                .values
                .keys()
                .any(|metric| outlier_metrics.iter().any(|m| m == metric))
        {
            return false;
        }
        if let Some(module) = query.module {
            let prefix = module.short_label();
            if !row.values.keys().any(|key| key.starts_with(prefix)) {
                return false;
            }
        }
        if let Some(metric) = &query.metric {
            if !row
                .values
                .keys()
                .any(|key| key.to_ascii_lowercase().contains(metric))
            {
                return false;
            }
        }
        if let Some(text) = &query.text {
            let haystack = format!(
                "{} {} {} {}",
                row.batch_name,
                row.sample_name,
                row.source_path.to_string_lossy(),
                row.values
                    .iter()
                    .map(|(k, v)| format!("{k} {}", v.display))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_ascii_lowercase();
            if !haystack.contains(text) {
                return false;
            }
        }
        true
    }
}

fn module_from_text(input: &str) -> Option<ImatestModule> {
    let lower = input.to_ascii_lowercase();
    ImatestModule::ALL.into_iter().find(|module| {
        lower.contains(&module.short_label().to_ascii_lowercase())
            || lower.contains(&format!("{module:?}").to_ascii_lowercase())
            || module
                .keywords()
                .iter()
                .any(|keyword| lower.contains(&keyword.to_ascii_lowercase()))
    })
}

fn status_from_text(input: &str) -> Option<EvaluationStatus> {
    let lower = input.to_ascii_lowercase();
    if lower.contains("失败") || lower.contains("fail") {
        Some(EvaluationStatus::Fail)
    } else if lower.contains("警告") || lower.contains("warn") {
        Some(EvaluationStatus::Warn)
    } else if lower.contains("通过") || lower.contains("pass") {
        Some(EvaluationStatus::Pass)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keyword_query() {
        let q = DataQueryService::parse("找出 MTF 失败项");
        assert_eq!(q.module, Some(ImatestModule::Mtf));
        assert_eq!(q.status, Some(EvaluationStatus::Fail));
    }
}
