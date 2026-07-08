//! 启动时数据一致性校验与自动修复。

use std::path::Path;

use rusqlite::Connection;

use crate::review::error::ReviewResult;
use crate::review::storage::paths::thumbnail_cache_dir;

/// 校验并修复无效条目（孤儿缩略图引用、缺失文件、计数不一致）。
pub fn repair(conn: &Connection) -> ReviewResult<ConsistencyReport> {
    let mut report = ConsistencyReport::default();
    report.missing_files = clear_missing_file_thumbnails(conn)?;
    report.orphan_annotations = delete_orphan_annotations(conn)?;
    report.recount_batches = recount_batch_totals(conn)?;
    report.recount_annotations = refresh_annotation_counts(conn)?;
    if report.total_fixes() > 0 {
        tracing::info!(?report, "review consistency repair completed");
    }
    Ok(report)
}

#[derive(Debug, Clone, Default)]
pub struct ConsistencyReport {
    pub missing_files: usize,
    pub orphan_annotations: usize,
    pub recount_batches: usize,
    pub recount_annotations: usize,
}

impl ConsistencyReport {
    pub fn total_fixes(&self) -> usize {
        self.missing_files
            + self.orphan_annotations
            + self.recount_batches
            + self.recount_annotations
    }
}

fn clear_missing_file_thumbnails(conn: &Connection) -> ReviewResult<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, thumbnail_path FROM review_image_item WHERE deleted_at IS NULL",
    )?;
    let rows: Vec<(i64, String, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut fixed = 0usize;
    for (id, file_path, thumb) in rows {
        if !Path::new(&file_path).exists() {
            conn.execute(
                "UPDATE review_image_item SET deleted_at = ?1 WHERE id = ?2",
                rusqlite::params![chrono::Utc::now().timestamp(), id],
            )?;
            fixed += 1;
            continue;
        }
        if let Some(t) = thumb {
            if !Path::new(&t).exists() {
                conn.execute(
                    "UPDATE review_image_item SET thumbnail_path = NULL WHERE id = ?1",
                    [id],
                )?;
                fixed += 1;
            }
        }
    }
    let _ = thumbnail_cache_dir();
    Ok(fixed)
}

fn delete_orphan_annotations(conn: &Connection) -> ReviewResult<usize> {
    let n = conn.execute(
    "DELETE FROM review_annotation WHERE image_item_id NOT IN (SELECT id FROM review_image_item)",
    [],
  )?;
    Ok(n)
}

fn recount_batch_totals(conn: &Connection) -> ReviewResult<usize> {
    let n = conn.execute(
        "UPDATE review_batch SET total_count = (
       SELECT COUNT(*) FROM review_image_item
       WHERE batch_id = review_batch.id AND deleted_at IS NULL
     )",
        [],
    )?;
    Ok(n)
}

fn refresh_annotation_counts(conn: &Connection) -> ReviewResult<usize> {
    let n = conn.execute(
        "UPDATE review_image_item SET annotation_count = (
       SELECT COUNT(*) FROM review_annotation WHERE image_item_id = review_image_item.id
     )",
        [],
    )?;
    Ok(n)
}
