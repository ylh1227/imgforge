//! 视频评审 SQLite 表结构（`user_version` 4）。

use rusqlite::Connection;

use crate::review::storage::migrate as review_migrate;
use crate::video_review::error::VideoReviewResult;

pub const VIDEO_REVIEW_SCHEMA_VERSION: i32 = 4;

const SCHEMA_V4: &str = r#"
CREATE TABLE IF NOT EXISTS video_review_batch (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  total_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS video_review_item (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  batch_id INTEGER NOT NULL,
  file_path TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL DEFAULT 'pending',
  remark TEXT,
  thumbnail_path TEXT,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  fps REAL NOT NULL DEFAULT 0,
  width INTEGER NOT NULL DEFAULT 0,
  height INTEGER NOT NULL DEFAULT 0,
  video_codec TEXT NOT NULL DEFAULT '',
  audio_codec TEXT,
  bitrate_kbps INTEGER,
  offset_ms INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_video_review_item_batch ON video_review_item(batch_id);
CREATE INDEX IF NOT EXISTS idx_video_review_item_status ON video_review_item(status);
CREATE INDEX IF NOT EXISTS idx_video_review_item_deleted ON video_review_item(deleted_at);

CREATE TABLE IF NOT EXISTS video_review_tag (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  color TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS video_review_item_tag (
  video_id INTEGER NOT NULL,
  tag_id INTEGER NOT NULL,
  PRIMARY KEY (video_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_video_review_item_tag_video ON video_review_item_tag(video_id);
CREATE INDEX IF NOT EXISTS idx_video_review_item_tag_tag ON video_review_item_tag(tag_id);

CREATE TABLE IF NOT EXISTS video_review_marker (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  video_id INTEGER NOT NULL,
  time_ms INTEGER NOT NULL,
  kind TEXT NOT NULL,
  text TEXT NOT NULL DEFAULT '',
  severity INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_video_review_marker_video ON video_review_marker(video_id);

CREATE TABLE IF NOT EXISTS video_review_segment (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  video_id INTEGER NOT NULL,
  start_ms INTEGER NOT NULL,
  end_ms INTEGER NOT NULL,
  text TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'pending',
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_video_review_segment_video ON video_review_segment(video_id);

CREATE TABLE IF NOT EXISTS video_review_compare_session (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

pub fn ensure_schema(conn: &Connection) -> VideoReviewResult<()> {
  review_migrate::ensure_schema(conn).map_err(|e| {
    crate::video_review::error::VideoReviewError::Message(e.to_string())
  })?;
  let version: i32 = conn
    .pragma_query_value(None, "user_version", |row| row.get(0))
    .unwrap_or(0);
  if version < VIDEO_REVIEW_SCHEMA_VERSION {
    conn.execute_batch(SCHEMA_V4)?;
    conn.pragma_update(None, "user_version", VIDEO_REVIEW_SCHEMA_VERSION)?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use rusqlite::Connection;

  #[test]
  fn video_schema_applies_on_fresh_db() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();
    let version: i32 = conn
      .pragma_query_value(None, "user_version", |row| row.get(0))
      .unwrap();
    assert_eq!(version, VIDEO_REVIEW_SCHEMA_VERSION);
    let count: i64 = conn
      .query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'video_review_item'",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(count, 1);
  }
}
