//! 汇总宽表领域模型。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::evaluation::EvaluationStatus;
use super::source_kind::SourceKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryColumn {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SummaryTableMode {
    Wide,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryRecordRef {
    pub batch_index: usize,
    pub record_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryCell {
    pub display: String,
    pub numeric: Option<f64>,
    pub status: EvaluationStatus,
    pub record_ref: SummaryRecordRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryRow {
    pub batch_index: usize,
    pub batch_id: String,
    pub batch_name: String,
    pub sample_name: String,
    pub source_path: PathBuf,
    pub source_kind: SourceKind,
    pub status: EvaluationStatus,
    pub warning_count: usize,
    pub conflict_count: usize,
    pub values: BTreeMap<String, SummaryCell>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SummaryTable {
    pub columns: Vec<SummaryColumn>,
    pub rows: Vec<SummaryRow>,
}

impl SummaryTable {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}
