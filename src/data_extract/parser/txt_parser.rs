//! TXT/INI/日志风格文本解析。

use std::fs;
use std::path::Path;

use crate::data_extract::domain::{ExtractionRecord, ImatestModule, MetricValue, ParseWarning};
use crate::data_extract::error::DataExtractResult;
use crate::data_extract::parser::aliases::map_field_to_metric;
use crate::data_extract::parser::module_detector::{
    detect_module_from_path, detect_module_from_text,
};

#[derive(Debug, Default)]
pub struct TxtParseOutput {
    pub records: Vec<ExtractionRecord>,
    pub warnings: Vec<ParseWarning>,
}

pub fn parse_txt_file(path: &Path) -> DataExtractResult<TxtParseOutput> {
    let text = fs::read_to_string(path)?;
    parse_txt_content(path, &text)
}

pub fn parse_txt_content(path: &Path, text: &str) -> DataExtractResult<TxtParseOutput> {
    let module = detect_module_from_path(path)
        .or_else(|| detect_module_from_text(text))
        .unwrap_or(ImatestModule::Mtf);

    let mut out = TxtParseOutput::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((name, value)) = split_kv(line) {
            if let Some(record) = field_to_record(module, name, value, path) {
                out.records.push(record);
            }
        }
    }

    if out.records.is_empty() {
        out.warnings.push(ParseWarning::new(
            "no_metrics",
            format!("TXT 未识别到 key=value 指标：{}", path.display()),
        ));
    }

    Ok(out)
}

fn split_kv(line: &str) -> Option<(&str, &str)> {
    if let Some((k, v)) = line.split_once('=') {
        return Some((k.trim(), v.trim()));
    }
    if let Some((k, v)) = line.split_once(':') {
        return Some((k.trim(), v.trim()));
    }
    None
}

fn field_to_record(
    module: ImatestModule,
    raw_name: &str,
    raw_value: &str,
    path: &Path,
) -> Option<ExtractionRecord> {
    if raw_value.is_empty() {
        return None;
    }
    let metric_key = map_field_to_metric(module, raw_name)
        .map(|k| k.to_string())
        .unwrap_or_else(|| raw_name.to_ascii_lowercase().replace(' ', "_"));

    let value = if let Ok(n) = raw_value.parse::<f64>() {
        MetricValue::number(n, "")
    } else {
        MetricValue::text(raw_value)
    };

    let mut record = ExtractionRecord::new(
        module,
        metric_key,
        raw_name,
        value,
        path.to_path_buf(),
        "txt",
    );
    if map_field_to_metric(module, raw_name).is_none() {
        record = record.with_warning(ParseWarning::new(
            "unknown_field",
            format!("未映射字段：{raw_name}"),
        ));
    }
    Some(record)
}

/// HTML 轻量提取：去标签后按 TXT 解析。
pub fn parse_html_file(path: &Path) -> DataExtractResult<TxtParseOutput> {
    let html = fs::read_to_string(path)?;
    let text = strip_html_tags(&html);
    parse_txt_content(path, &text)
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_txt_kv() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "MTF50 = 0.44").unwrap();
        writeln!(f, "SNR (dB): 36.5").unwrap();
        f.flush().unwrap();
        let out = parse_txt_file(f.path()).unwrap();
        assert!(out.records.iter().any(|r| r.metric_key == "mtf50"));
    }
}
