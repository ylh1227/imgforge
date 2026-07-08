//! JSON 解析（扁平化 key path）。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::data_extract::domain::{ExtractionRecord, ImatestModule, MetricValue, ParseWarning};
use crate::data_extract::error::DataExtractResult;
use crate::data_extract::parser::aliases::map_field_to_metric;
use crate::data_extract::parser::module_detector::{
    detect_module_from_path, detect_module_from_text,
};

#[derive(Debug, Default)]
pub struct JsonParseOutput {
    pub records: Vec<ExtractionRecord>,
    pub warnings: Vec<ParseWarning>,
}

pub fn parse_json_file(path: &Path) -> DataExtractResult<JsonParseOutput> {
    let text = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    let flat = flatten_json("", &value);
    let blob: String = flat.keys().cloned().collect::<Vec<_>>().join(" ");
    let module = detect_module_from_path(path)
        .or_else(|| detect_module_from_text(&blob))
        .unwrap_or(ImatestModule::Mtf);

    let mut out = JsonParseOutput::default();
    for (key, val) in flat {
        let raw_value = json_value_to_string(&val);
        if raw_value.is_empty() {
            continue;
        }
        let leaf = key.rsplit('.').next().unwrap_or(&key);
        let metric_key = map_field_to_metric(module, leaf)
            .map(|k| k.to_string())
            .unwrap_or_else(|| leaf.to_ascii_lowercase().replace('.', "_"));

        let mut record = ExtractionRecord::new(
            module,
            metric_key,
            leaf,
            parse_value(&raw_value),
            path.to_path_buf(),
            "json",
        );
        if map_field_to_metric(module, leaf).is_none() {
            record = record.with_warning(ParseWarning::new(
                "unknown_field",
                format!("未映射字段：{leaf}"),
            ));
        }
        out.records.push(record);
    }

    if out.records.is_empty() {
        out.warnings.push(ParseWarning::new(
            "no_metrics",
            format!("JSON 未识别到指标：{}", path.display()),
        ));
    }

    Ok(out)
}

fn flatten_json(prefix: &str, value: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if v.is_object() {
                    out.extend(flatten_json(&key, v));
                } else {
                    out.insert(key, v.clone());
                }
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let key = format!("{prefix}[{i}]");
                if v.is_object() || v.is_array() {
                    out.extend(flatten_json(&key, v));
                } else {
                    out.insert(key, v.clone());
                }
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string(), value.clone());
            }
        }
    }
    out
}

fn json_value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn parse_value(raw: &str) -> MetricValue {
    if let Ok(n) = raw.parse::<f64>() {
        MetricValue::number(n, "")
    } else {
        MetricValue::text(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_nested_json() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"{{"module":"mtf","results":{{"MTF50":0.42,"MTF30":0.31}}}}"#
        )
        .unwrap();
        f.flush().unwrap();
        let out = parse_json_file(f.path()).unwrap();
        assert!(out.records.iter().any(|r| r.metric_key == "mtf50"));
    }
}
