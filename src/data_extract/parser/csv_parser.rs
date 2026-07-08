//! CSV/TSV 解析。

use std::fs::File;
use std::path::Path;

use crate::data_extract::domain::{ExtractionRecord, ImatestModule, MetricValue, ParseWarning};
use crate::data_extract::error::{DataExtractError, DataExtractResult};
use crate::data_extract::parser::aliases::map_field_to_metric;
use crate::data_extract::parser::module_detector::{
    detect_module_from_path, detect_module_from_text,
};

#[derive(Debug, Default)]
pub struct CsvParseOutput {
    pub records: Vec<ExtractionRecord>,
    pub warnings: Vec<ParseWarning>,
}

pub fn parse_csv_file(path: &Path) -> DataExtractResult<CsvParseOutput> {
    let mut file = File::open(path)?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(&mut file);

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| DataExtractError::ParseFailed {
            path: path.to_path_buf(),
            detail: e.to_string(),
        })?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let header_blob = headers.join(" ");
    let module = detect_module_from_path(path)
        .or_else(|| detect_module_from_text(&header_blob))
        .unwrap_or(ImatestModule::Mtf);

    let mut out = CsvParseOutput::default();
    for result in reader.records() {
        let row = result.map_err(|e| DataExtractError::ParseFailed {
            path: path.to_path_buf(),
            detail: e.to_string(),
        })?;
        let sample = row.get(0).map(|s| s.to_string());
        for (idx, header) in headers.iter().enumerate() {
            if idx == 0 && looks_like_sample_column(header) {
                continue;
            }
            let raw = row.get(idx).unwrap_or("").trim();
            if raw.is_empty() {
                continue;
            }
            if let Some(record) = field_to_record(module, header, raw, path, sample.as_deref()) {
                out.records.push(record);
            }
        }
    }

    if out.records.is_empty() {
        out.warnings.push(ParseWarning::new(
            "no_metrics",
            format!("CSV 未识别到指标：{}", path.display()),
        ));
    }

    Ok(out)
}

fn looks_like_sample_column(header: &str) -> bool {
    let h = header.to_ascii_lowercase();
    h.contains("sample")
        || h.contains("image")
        || h.contains("file")
        || h.contains("name")
        || h == "roi"
}

fn field_to_record(
    module: ImatestModule,
    raw_name: &str,
    raw_value: &str,
    path: &Path,
    sample: Option<&str>,
) -> Option<ExtractionRecord> {
    let metric_key = map_field_to_metric(module, raw_name)
        .map(|k| k.to_string())
        .unwrap_or_else(|| normalize_key(raw_name));

    let value = parse_value(raw_value);
    let mut record = ExtractionRecord::new(
        module,
        metric_key,
        raw_name,
        value,
        path.to_path_buf(),
        "csv",
    );
    if let Some(s) = sample.filter(|s| !s.is_empty()) {
        record = record.with_sample(s);
    }
    if map_field_to_metric(module, raw_name).is_none() {
        record = record.with_warning(ParseWarning::new(
            "unknown_field",
            format!("未映射字段：{raw_name}"),
        ));
    }
    Some(record)
}

fn normalize_key(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '(', ')', '%', '/'], "_")
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn parse_value(raw: &str) -> MetricValue {
    let trimmed = raw.trim();
    if let Ok(n) = trimmed.parse::<f64>() {
        return MetricValue::number(n, "");
    }
    if let Some((num, unit)) = split_number_unit(trimmed) {
        if let Ok(n) = num.parse::<f64>() {
            return MetricValue::number(n, unit);
        }
    }
    MetricValue::text(trimmed)
}

fn split_number_unit(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    let mut split_at = None;
    for (i, ch) in s.char_indices() {
        if ch.is_alphabetic() && i > 0 {
            split_at = Some(i);
            break;
        }
    }
    split_at.map(|i| s.split_at(i))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_mtf_csv() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "ROI,MTF50,MTF30,SNR (dB)").unwrap();
        writeln!(f, "Center,0.45,0.32,38.2").unwrap();
        f.flush().unwrap();
        let out = parse_csv_file(f.path()).unwrap();
        assert!(!out.records.is_empty());
        assert!(out.records.iter().any(|r| r.metric_key == "mtf50"));
    }
}
