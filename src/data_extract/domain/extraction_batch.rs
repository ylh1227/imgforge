//! 提取批次。

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::evaluation::EvaluationSummary;
use super::extraction_record::ExtractionRecord;
use super::imatest_module::ImatestModule;
use super::parse_warning::ParseWarning;
use super::threshold::ThresholdProfile;
use super::unmapped::{collect_unmapped_fields, UnmappedFieldStat};

/// 一次导入目录或文件集合。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractionBatch {
    pub id: String,
    pub name: String,
    pub source_root: PathBuf,
    pub created_at: DateTime<Utc>,
    pub records: Vec<ExtractionRecord>,
    pub warnings: Vec<ParseWarning>,
    pub files_scanned: usize,
    pub files_parsed: usize,
    #[serde(default)]
    pub unmapped_fields: Vec<UnmappedFieldStat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_summary: Option<EvaluationSummary>,
}

impl ExtractionBatch {
    pub fn new(name: impl Into<String>, source_root: PathBuf) -> Self {
        let now = Utc::now();
        let id = format!("batch-{}", now.timestamp_millis());
        Self {
            id,
            name: name.into(),
            source_root,
            created_at: now,
            records: Vec::new(),
            warnings: Vec::new(),
            files_scanned: 0,
            files_parsed: 0,
            unmapped_fields: Vec::new(),
            evaluation_summary: None,
        }
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn modules_found(&self) -> Vec<ImatestModule> {
        let mut mods: Vec<ImatestModule> = self.records.iter().map(|r| r.module).collect();
        mods.sort_by_key(|m| format!("{:?}", m));
        mods.dedup();
        mods
    }

    pub fn summary_line(&self) -> String {
        let eval = self
            .evaluation_summary
            .as_ref()
            .map(|s| format!("，通过 {} / 警告 {} / 失败 {}", s.pass, s.warn, s.fail))
            .unwrap_or_default();
        format!(
            "{} 条指标，{} 个模块，扫描 {} 个文件，解析 {}{}",
            self.record_count(),
            self.modules_found().len(),
            self.files_scanned,
            self.files_parsed,
            eval
        )
    }

    pub fn apply_thresholds(&mut self, profile: &ThresholdProfile) {
        for rec in &mut self.records {
            let eval = profile.evaluate(rec);
            rec.evaluation = Some(eval);
        }
        self.evaluation_summary = Some(EvaluationSummary::from_statuses(
            self.records.iter().map(|r| r.evaluation_status()),
        ));
        self.unmapped_fields = collect_unmapped_fields(&self.records);
    }

    pub fn refresh_unmapped(&mut self) {
        self.unmapped_fields = collect_unmapped_fields(&self.records);
    }
}
