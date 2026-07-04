//! 评审表结构初始化与 `user_version` 迁移。

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::review::domain::annotation::AnnotationKind;
use crate::review::domain::image_item::ReviewStatus;
use crate::review::error::ReviewResult;

/// 当前评审 schema 版本（独立于主库其它模块时可共用同一 `user_version`，此处仅管理评审表）。
pub const REVIEW_SCHEMA_VERSION: i32 = 1;

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

/// 在已有连接上初始化/升级评审表结构。
pub fn ensure_schema(conn: &Connection) -> ReviewResult<()> {
  conn.pragma_update(None, "foreign_keys", "ON")?;
  let version: i32 = conn
    .pragma_query_value(None, "user_version", |row| row.get(0))
    .unwrap_or(0);

  if version >= REVIEW_SCHEMA_VERSION {
    return Ok(());
  }

  if table_exists(conn, "review_batch")? && is_legacy_schema(conn)? {
    migrate_v0_to_v1(conn)?;
  } else {
    conn.execute_batch(SCHEMA_V1)?;
  }

  conn.pragma_update(None, "user_version", REVIEW_SCHEMA_VERSION)?;
  Ok(())
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
    let mut stmt = conn.prepare(
      "SELECT id, name, total_count, created_at, updated_at FROM review_batch",
    )?;
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
  fn fresh_db_gets_v1_schema() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();
    let version: i32 = conn
      .pragma_query_value(None, "user_version", |row| row.get(0))
      .unwrap();
    assert_eq!(version, REVIEW_SCHEMA_VERSION);
    let ty: String = conn
      .query_row(
        "SELECT type FROM pragma_table_info('review_image_item') WHERE name = 'status'",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(ty, "TEXT");
  }
}
