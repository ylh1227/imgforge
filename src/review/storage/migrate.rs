//! 评审表结构初始化与 `user_version` 迁移。

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::review::domain::annotation::AnnotationKind;
use crate::review::domain::image_item::ReviewStatus;
use crate::review::error::ReviewResult;

/// 当前评审 schema 版本。
pub const REVIEW_SCHEMA_VERSION: i32 = 4;

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS review_batch (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  total_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS review_image_item (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  batch_id INTEGER NOT NULL,
  file_path TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL DEFAULT 'pending',
  remark TEXT,
  thumbnail_path TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_review_image_item_batch_id ON review_image_item(batch_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_review_image_item_file_path ON review_image_item(file_path);

CREATE TABLE IF NOT EXISTS review_annotation (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  image_item_id INTEGER NOT NULL,
  anno_type TEXT NOT NULL,
  position TEXT NOT NULL,
  style TEXT NOT NULL,
  content TEXT,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_review_annotation_image_item_id ON review_annotation(image_item_id);

CREATE TABLE IF NOT EXISTS review_session (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

const SCHEMA_V2_EXTRA: &str = r#"
CREATE TABLE IF NOT EXISTS review_custom_status (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  color TEXT NOT NULL,
  maps_to TEXT,
  convert_params TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_review_image_item_status ON review_image_item(status);
CREATE INDEX IF NOT EXISTS idx_review_image_item_batch_status ON review_image_item(batch_id, status);
CREATE INDEX IF NOT EXISTS idx_review_image_item_updated ON review_image_item(updated_at);
CREATE INDEX IF NOT EXISTS idx_review_image_item_deleted ON review_image_item(deleted_at);
CREATE INDEX IF NOT EXISTS idx_review_annotation_z ON review_annotation(image_item_id, z_index);
"#;

const SCHEMA_V3_EXTRA: &str = r#"
CREATE TABLE IF NOT EXISTS review_tag (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  color TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS review_image_tag (
  image_item_id INTEGER NOT NULL,
  tag_id INTEGER NOT NULL,
  PRIMARY KEY (image_item_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_review_image_tag_image ON review_image_tag(image_item_id);
CREATE INDEX IF NOT EXISTS idx_review_image_tag_tag ON review_image_tag(tag_id);
"#;

/// 在已有连接上初始化/升级评审表结构。
pub fn ensure_schema(conn: &Connection) -> ReviewResult<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "NORMAL");

    let mut version: i32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap_or(0);

    if version == 0 {
        if table_exists(conn, "review_batch")? && is_legacy_schema(conn)? {
            migrate_v0_to_v1(conn)?;
        } else {
            conn.execute_batch(SCHEMA_V1)?;
        }
        version = 1;
    }

    if version < 2 {
        migrate_v1_to_v2(conn)?;
        version = 2;
    }

    if version < 3 {
        migrate_v2_to_v3(conn)?;
        version = 3;
    }

    if version < 4 {
        migrate_v3_to_v4(conn)?;
        version = 4;
    }

    let _ = version;
    conn.pragma_update(None, "user_version", REVIEW_SCHEMA_VERSION)?;
    Ok(())
}

fn migrate_v3_to_v4(conn: &Connection) -> ReviewResult<()> {
    add_column_if_missing(conn, "review_image_item", "jira_issue_key", "TEXT")?;
    add_column_if_missing(conn, "review_image_item", "jira_url", "TEXT")?;
    Ok(())
}

fn migrate_v2_to_v3(conn: &Connection) -> ReviewResult<()> {
    conn.execute_batch(SCHEMA_V3_EXTRA)?;
    Ok(())
}

fn migrate_v1_to_v2(conn: &Connection) -> ReviewResult<()> {
    add_column_if_missing(conn, "review_batch", "deleted_at", "INTEGER")?;
    add_column_if_missing(conn, "review_image_item", "deleted_at", "INTEGER")?;
    add_column_if_missing(conn, "review_image_item", "file_size", "INTEGER")?;
    add_column_if_missing(conn, "review_image_item", "width", "INTEGER")?;
    add_column_if_missing(conn, "review_image_item", "height", "INTEGER")?;
    add_column_if_missing(conn, "review_image_item", "convert_format", "TEXT")?;
    add_column_if_missing(conn, "review_image_item", "convert_quality", "INTEGER")?;
    add_column_if_missing(conn, "review_image_item", "convert_width", "INTEGER")?;
    add_column_if_missing(
        conn,
        "review_image_item",
        "annotation_count",
        "INTEGER DEFAULT 0",
    )?;
    add_column_if_missing(conn, "review_annotation", "locked", "INTEGER DEFAULT 0")?;
    add_column_if_missing(conn, "review_annotation", "z_index", "INTEGER DEFAULT 0")?;
    conn.execute_batch(SCHEMA_V2_EXTRA)?;
    conn.execute(
        "UPDATE review_image_item SET annotation_count = (
       SELECT COUNT(*) FROM review_annotation WHERE image_item_id = review_image_item.id
     ) WHERE annotation_count IS NULL OR annotation_count = 0",
        [],
    )?;
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    col_type: &str,
) -> ReviewResult<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {col_type}");
    conn.execute(&sql, [])?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> ReviewResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn table_exists(conn: &Connection, name: &str) -> ReviewResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn is_legacy_schema(conn: &Connection) -> ReviewResult<bool> {
    if !table_exists(conn, "review_image_item")? {
        return Ok(false);
    }
    let mut stmt = conn.prepare("PRAGMA table_info(review_image_item)")?;
    let rows = stmt.query_map([], |row| {
        let col_name: String = row.get(1)?;
        let col_type: String = row.get(2)?;
        Ok((col_name, col_type))
    })?;
    for row in rows {
        let (name, ty) = row?;
        if name == "status" && ty.eq_ignore_ascii_case("INTEGER") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_v0_to_v1(conn: &Connection) -> ReviewResult<()> {
    #[derive(Clone)]
    struct LegacyBatch {
        id: i64,
        name: String,
        total_count: i32,
        created_at: String,
        updated_at: String,
    }

    #[derive(Clone)]
    struct LegacyImage {
        id: i64,
        batch_id: i64,
        file_path: String,
        status: i32,
        remark: String,
        thumbnail_path: Option<String>,
        created_at: String,
        updated_at: String,
    }

    #[derive(Clone)]
    struct LegacyAnnotation {
        image_item_id: i64,
        kind: i32,
        position: String,
        style: String,
        content: String,
        created_at: String,
    }

    let batches: Vec<LegacyBatch> = {
        let mut stmt =
            conn.prepare("SELECT id, name, total_count, created_at, updated_at FROM review_batch")?;
        let rows = stmt.query_map([], |row| {
            Ok(LegacyBatch {
                id: row.get(0)?,
                name: row.get(1)?,
                total_count: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let images: Vec<LegacyImage> = {
        let mut stmt = conn.prepare(
            "SELECT id, batch_id, file_path, status, remark, thumbnail_path, created_at, updated_at
       FROM review_image_item",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LegacyImage {
                id: row.get(0)?,
                batch_id: row.get(1)?,
                file_path: row.get(2)?,
                status: row.get(3)?,
                remark: row.get(4)?,
                thumbnail_path: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let annotations: Vec<LegacyAnnotation> = {
        let mut stmt = conn.prepare(
      "SELECT image_item_id, type, position, style, content, created_at FROM review_annotation",
    )?;
        let rows = stmt.query_map([], |row| {
            Ok(LegacyAnnotation {
                image_item_id: row.get(0)?,
                kind: row.get(1)?,
                position: row.get(2)?,
                style: row.get(3)?,
                content: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "DROP TABLE IF EXISTS review_annotation;
     DROP TABLE IF EXISTS review_image_item;
     DROP TABLE IF EXISTS review_batch;",
    )?;
    tx.execute_batch(SCHEMA_V1)?;

    for batch in batches {
        tx.execute(
            "INSERT INTO review_batch (id, name, total_count, created_at, updated_at)
       VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                batch.id,
                batch.name,
                batch.total_count,
                parse_legacy_ts(&batch.created_at),
                parse_legacy_ts(&batch.updated_at),
            ],
        )?;
    }

    for image in images {
        let status = ReviewStatus::from_db(image.status)
            .unwrap_or(ReviewStatus::Pending)
            .to_sql();
        tx.execute(
            "INSERT INTO review_image_item
       (id, batch_id, file_path, status, remark, thumbnail_path, created_at, updated_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                image.id,
                image.batch_id,
                image.file_path,
                status,
                if image.remark.is_empty() {
                    None::<String>
                } else {
                    Some(image.remark)
                },
                image.thumbnail_path,
                parse_legacy_ts(&image.created_at),
                parse_legacy_ts(&image.updated_at),
            ],
        )?;
    }

    for ann in annotations {
        let anno_type = AnnotationKind::from_db(ann.kind)
            .unwrap_or(AnnotationKind::Rectangle)
            .to_sql();
        tx.execute(
            "INSERT INTO review_annotation
       (image_item_id, anno_type, position, style, content, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                ann.image_item_id,
                anno_type,
                ann.position,
                ann.style,
                if ann.content.is_empty() {
                    None::<String>
                } else {
                    Some(ann.content)
                },
                parse_legacy_ts(&ann.created_at),
            ],
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn parse_legacy_ts(raw: &str) -> i64 {
    if let Ok(ts) = raw.parse::<i64>() {
        return ts;
    }
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|_| Utc::now().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn fresh_db_gets_latest_schema() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, REVIEW_SCHEMA_VERSION);
        assert!(column_exists(&conn, "review_image_item", "annotation_count").unwrap());
        assert!(column_exists(&conn, "review_image_item", "jira_issue_key").unwrap());
        assert!(table_exists(&conn, "review_tag").unwrap());
        assert!(table_exists(&conn, "review_image_tag").unwrap());
    }
}
