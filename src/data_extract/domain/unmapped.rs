//! 未映射字段统计。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::extraction_record::ExtractionRecord;
use super::imatest_module::ImatestModule;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmappedFieldStat {
    pub module: ImatestModule,
    pub raw_name: String,
    pub count: usize,
}

pub fn collect_unmapped_fields(records: &[ExtractionRecord]) -> Vec<UnmappedFieldStat> {
    let mut map: BTreeMap<(ImatestModule, String), usize> = BTreeMap::new();
    for rec in records {
        if rec.warnings.iter().any(|w| w.code == "unknown_field") {
            *map.entry((rec.module, rec.raw_name.clone())).or_insert(0) += 1;
        }
    }
    map.into_iter()
        .map(|((module, raw_name), count)| UnmappedFieldStat {
            module,
            raw_name,
            count,
        })
        .collect()
}
