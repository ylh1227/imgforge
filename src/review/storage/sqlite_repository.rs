//! rusqlite 实现的评审仓储：共用 `Connection`，通过 `ReviewRepository` trait 对外暴露。

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::review::domain::annotation::{
  Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle,
};
use crate::review::domain::batch::{BatchStats, ReviewBatch};
use crate::review::domain::image_item::{ImageFilter, ReviewImageItem, ReviewStatus};
use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::migrate;
use crate::review::storage::paths::{app_data_dir, database_path};
use crate::review::storage::repository::{ItemFilter, NewReviewImageItem, ReviewRepository};

/// SQLite 评审仓储（持有或借用主项目同一连接）。
pub struct SqliteReviewRepository {
  conn: Connection,
}

impl SqliteReviewRepository {
  /// 使用已有连接初始化评审表（与主项目共用同一 `Connection`）。
  pub fn new(conn: Connection) -> ReviewResult<Self> {
    migrate::ensure_schema(&conn)?;
    Ok(Self { conn })
  }

  /// 仅初始化评审表结构，不转移连接所有权。
  pub fn init(conn: &Connection) -> ReviewResult<()> {
    migrate::ensure_schema(conn)
  }

  /// 打开（或创建）独立数据库文件并初始化评审表。
  pub fn open() -> ReviewResult<Self> {
    let path = database_path()?;
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    Self::new(conn)
  }

  /// 内存数据库（单元测试用）。
  pub fn open_memory() -> ReviewResult<Self> {
    let conn = Connection::open_in_memory()?;
    Self::new(conn)
  }

  pub fn connection(&self) -> &Connection {
    &self.conn
  }

  // ── 扩展方法（service / UI 层使用，不在 trait 中）────────────────

  /// 创建批次并批量导入图片路径。
  pub fn create_batch(&self, name: &str, file_paths: &[PathBuf]) -> ReviewResult<i64> {
    let batch_id = ReviewRepository::create_batch(self, name)?;
    if file_paths.is_empty() {
      return Ok(batch_id);
    }
    let items: Vec<NewReviewImageItem> = file_paths
      .iter()
      .map(|p| NewReviewImageItem {
        file_path: p.clone(),
        thumbnail_path: None,
      })
      .collect();
    ReviewRepository::add_image_items(self, batch_id, &items)?;
    Ok(batch_id)
  }

  pub fn list_images(
    &self,
    batch_id: i64,
    filter: &ImageFilter,
  ) -> ReviewResult<Vec<ReviewImageItem>> {
    let mut items =
      ReviewRepository::list_image_items(self, batch_id, ItemFilter::from(filter))?;
    if !filter.include_deleted {
      items.retain(|i| !i.is_deleted());
    }
    filter.apply_in_memory(&mut items);
    Ok(items)
  }

  pub fn get_image(&self, id: i64) -> ReviewResult<ReviewImageItem> {
    self
      .conn
      .query_row(
        "SELECT id, batch_id, file_path, status, remark, thumbnail_path, created_at, updated_at,
                deleted_at, file_size, width, height, convert_format, convert_quality, convert_width,
                annotation_count
         FROM review_image_item WHERE id = ?1",
        [id],
        map_image_row,
      )
      .optional()?
      .ok_or(ReviewError::NotFound {
        entity: "review_image_item",
        id,
      })
  }

  pub fn get_images_by_ids(&self, ids: &[i64]) -> ReviewResult<Vec<ReviewImageItem>> {
    if ids.is_empty() {
      return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
      out.push(self.get_image(*id)?);
    }
    Ok(out)
  }

  pub fn batch_stats(&self, batch_id: i64) -> ReviewResult<BatchStats> {
    let mut stmt = self.conn.prepare(
      "SELECT status, COUNT(*) FROM review_image_item WHERE batch_id = ?1 GROUP BY status",
    )?;
    let mut stats = BatchStats::default();
    let rows = stmt.query_map([batch_id], |row| {
      Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
    })?;
    for row in rows {
      let (status_raw, count) = row?;
      if let Some(s) = ReviewStatus::from_sql(&status_raw) {
        match s {
          ReviewStatus::Pending => stats.pending = count,
          ReviewStatus::Approved => stats.approved = count,
          ReviewStatus::NeedsFix => stats.needs_fix = count,
          ReviewStatus::Rejected => stats.rejected = count,
        }
      }
    }
    Ok(stats)
  }

  pub fn list_batches(&self) -> ReviewResult<Vec<ReviewBatch>> {
    ReviewRepository::list_batches(self)
  }

  pub fn list_annotations(&self, image_item_id: i64) -> ReviewResult<Vec<Annotation>> {
    ReviewRepository::list_annotations(self, image_item_id)
  }

  pub fn update_image_status(&self, item_id: i64, status: ReviewStatus) -> ReviewResult<()> {
    ReviewRepository::update_image_status(self, item_id, status)
  }

  pub fn update_image_remark(&self, item_id: i64, remark: &str) -> ReviewResult<()> {
    ReviewRepository::update_image_remark(self, item_id, remark)
  }

  pub fn delete_annotation(&self, annotation_id: i64) -> ReviewResult<()> {
    ReviewRepository::delete_annotation(self, annotation_id)
  }

  pub fn set_thumbnail_path(&self, id: i64, path: &Path) -> ReviewResult<()> {
    self.conn.execute(
      "UPDATE review_image_item SET thumbnail_path = ?1 WHERE id = ?2",
      params![path.to_string_lossy().as_ref(), id],
    )?;
    Ok(())
  }

  pub fn batch_set_status(&self, ids: &[i64], status: ReviewStatus) -> ReviewResult<()> {
    ReviewRepository::batch_update_status(self, ids, status)?;
    Ok(())
  }

  pub fn batch_set_remarks(
    &self,
    ids: &[i64],
    text: &str,
    mode: crate::review::storage::traits::RemarkWriteMode,
  ) -> ReviewResult<()> {
    if ids.is_empty() {
      return Ok(());
    }
    use crate::review::storage::traits::RemarkWriteMode;
    let tx = self.conn.unchecked_transaction()?;
    let now = now_ts();
    for id in ids {
      let new_remark = match mode {
        RemarkWriteMode::Overwrite => text.to_string(),
        RemarkWriteMode::Append => {
          let current: Option<String> = tx.query_row(
            "SELECT remark FROM review_image_item WHERE id = ?1",
            [id],
            |row| row.get(0),
          )?;
          match current {
            None => text.to_string(),
            Some(ref s) if s.is_empty() => text.to_string(),
            Some(current) => format!("{current}\n{text}"),
          }
        }
      };
      let affected = tx.execute(
        "UPDATE review_image_item SET remark = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_remark, now, id],
      )?;
      if affected == 0 {
        return Err(ReviewError::NotFound {
          entity: "review_image_item",
          id: *id,
        });
      }
    }
    tx.commit()?;
    Ok(())
  }

  pub fn batch_insert_annotation_template(
    &self,
    template: &crate::review::storage::traits::AnnotationTemplate,
    image_ids: &[i64],
  ) -> ReviewResult<Vec<i64>> {
    if image_ids.is_empty() {
      return Ok(Vec::new());
    }
    let ann = Annotation {
      id: 0,
      image_item_id: 0,
      kind: template.kind,
      position: template.position.clone(),
      style: template.style.clone(),
      content: template.content.clone(),
      created_at: Utc::now(),
      locked: false,
      z_index: 0,
    };
    let tx = self.conn.unchecked_transaction()?;
    let mut inserted = Vec::with_capacity(image_ids.len());
    for image_id in image_ids {
      let pos = serde_json::to_string(&ann.position)?;
      let style = serde_json::to_string(&ann.style)?;
      tx.execute(
        "INSERT INTO review_annotation
         (image_item_id, anno_type, position, style, content, created_at, locked, z_index)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0)",
        params![
          image_id,
          ann.kind.to_sql(),
          pos,
          style,
          if ann.content.is_empty() {
            None::<String>
          } else {
            Some(ann.content.clone())
          },
          ann.created_at.timestamp(),
        ],
      )?;
      let id = tx.last_insert_rowid();
      tx.execute(
        "UPDATE review_image_item SET annotation_count = annotation_count + 1 WHERE id = ?1",
        [image_id],
      )?;
      inserted.push(id);
    }
    tx.commit()?;
    Ok(inserted)
  }

  pub fn delete_annotations_by_ids(&self, ids: &[i64]) -> ReviewResult<()> {
    if ids.is_empty() {
      return Ok(());
    }
    let tx = self.conn.unchecked_transaction()?;
    for id in ids {
      let affected = tx.execute("DELETE FROM review_annotation WHERE id = ?1", [id])?;
      if affected == 0 {
        return Err(ReviewError::NotFound {
          entity: "review_annotation",
          id: *id,
        });
      }
    }
    tx.commit()?;
    Ok(())
  }

  pub fn batch_clear_annotations(&self, image_ids: &[i64]) -> ReviewResult<()> {
    if image_ids.is_empty() {
      return Ok(());
    }
    let tx = self.conn.unchecked_transaction()?;
    for id in image_ids {
      tx.execute(
        "DELETE FROM review_annotation WHERE image_item_id = ?1",
        [id],
      )?;
    }
    tx.commit()?;
    Ok(())
  }

  pub fn insert_annotation(&self, ann: &Annotation) -> ReviewResult<i64> {
    ReviewRepository::add_annotation(self, ann.image_item_id, ann)
  }

  pub fn delete_last_annotation(&self, image_item_id: i64) -> ReviewResult<()> {
    self.conn.execute(
      "DELETE FROM review_annotation WHERE id = (
         SELECT id FROM review_annotation WHERE image_item_id = ?1 ORDER BY id DESC LIMIT 1
       )",
      [image_item_id],
    )?;
    Ok(())
  }

  pub fn update_annotation_position(
    &self,
    id: i64,
    position: &AnnotationPosition,
  ) -> ReviewResult<()> {
    let pos = serde_json::to_string(position)?;
    self.conn.execute(
      "UPDATE review_annotation SET position = ?1 WHERE id = ?2",
      params![pos, id],
    )?;
    Ok(())
  }

  pub fn update_annotation_content(&self, id: i64, content: &str) -> ReviewResult<()> {
    self.conn.execute(
      "UPDATE review_annotation SET content = ?1 WHERE id = ?2",
      params![content, id],
    )?;
    Ok(())
  }

  pub fn approved_paths_in_batch(&self, batch_id: i64) -> ReviewResult<Vec<PathBuf>> {
    let mut stmt = self.conn.prepare(
      "SELECT file_path FROM review_image_item
       WHERE batch_id = ?1 AND status = ?2 ORDER BY file_path",
    )?;
    let rows = stmt.query_map(
      params![batch_id, ReviewStatus::Approved.to_sql()],
      |row| row.get::<_, String>(0).map(PathBuf::from),
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(ReviewError::from)
  }

  pub fn status_for_path(&self, path: &Path) -> ReviewResult<Option<ReviewStatus>> {
    let p = path.to_string_lossy();
    let status: Option<String> = self
      .conn
      .query_row(
        "SELECT status FROM review_image_item WHERE file_path = ?1",
        [p.as_ref()],
        |row| row.get(0),
      )
      .optional()?;
    Ok(status.and_then(|s| ReviewStatus::from_sql(&s)))
  }

  pub fn list_export_rows(
    &self,
    query: &crate::review::storage::traits::ExportRowsQuery,
  ) -> ReviewResult<Vec<crate::review::storage::traits::ReviewExportRow>> {
    let filter = ItemFilter {
      status: query.status_filter,
      search: String::new(),
    };
    let mut items = ReviewRepository::list_image_items(self, query.batch_id, filter)?;
    if let Some(ref ids) = query.image_ids {
      let set: std::collections::HashSet<i64> = ids.iter().copied().collect();
      items.retain(|i| set.contains(&i.id));
    }
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
      let ann_count: i32 = self.conn.query_row(
        "SELECT COUNT(*) FROM review_annotation WHERE image_item_id = ?1",
        [item.id],
        |row| row.get(0),
      )?;
      let file_name = item
        .file_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| item.file_path.display().to_string());
      rows.push(crate::review::storage::traits::ReviewExportRow {
        file_name,
        file_path: item.file_path.to_string_lossy().to_string(),
        status: item.status,
        remark: item.remark,
        annotation_count: ann_count,
        updated_at: item.updated_at.to_rfc3339(),
      });
    }
    Ok(rows)
  }

  pub fn save_session(&self, batch_id: i64, image_id: i64) -> ReviewResult<()> {
    self.conn.execute(
      "INSERT INTO review_session (key, value) VALUES ('last_batch_id', ?1)
       ON CONFLICT(key) DO UPDATE SET value = excluded.value",
      params![batch_id.to_string()],
    )?;
    self.conn.execute(
      "INSERT INTO review_session (key, value) VALUES ('last_image_id', ?1)
       ON CONFLICT(key) DO UPDATE SET value = excluded.value",
      params![image_id.to_string()],
    )?;
    Ok(())
  }

  pub fn load_session(&self) -> ReviewResult<(Option<i64>, Option<i64>)> {
    let batch: Option<String> = self
      .conn
      .query_row(
        "SELECT value FROM review_session WHERE key = 'last_batch_id'",
        [],
        |row| row.get(0),
      )
      .optional()?;
    let image: Option<String> = self
      .conn
      .query_row(
        "SELECT value FROM review_session WHERE key = 'last_image_id'",
        [],
        |row| row.get(0),
      )
      .optional()?;
    Ok((
      batch.and_then(|s| s.parse().ok()),
      image.and_then(|s| s.parse().ok()),
    ))
  }

  pub fn soft_delete_image(&self, id: i64) -> ReviewResult<()> {
    let now = now_ts();
    let n = self.conn.execute(
      "UPDATE review_image_item SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
      params![now, id],
    )?;
    if n == 0 {
      return Err(ReviewError::NotFound {
        entity: "review_image_item",
        id,
      });
    }
    Ok(())
  }

  pub fn restore_image(&self, id: i64) -> ReviewResult<()> {
    let now = now_ts();
    self.conn.execute(
      "UPDATE review_image_item SET deleted_at = NULL, updated_at = ?1 WHERE id = ?2",
      params![now, id],
    )?;
    Ok(())
  }

  pub fn soft_delete_batch(&self, id: i64) -> ReviewResult<()> {
    let now = now_ts();
    let tx = self.conn.unchecked_transaction()?;
    tx.execute(
      "UPDATE review_batch SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
      params![now, id],
    )?;
    tx.execute(
      "UPDATE review_image_item SET deleted_at = ?1, updated_at = ?1 WHERE batch_id = ?2",
      params![now, id],
    )?;
    tx.commit()?;
    Ok(())
  }

  pub fn list_deleted_images(&self, batch_id: i64) -> ReviewResult<Vec<ReviewImageItem>> {
    let mut stmt = self.conn.prepare(
      "SELECT id, batch_id, file_path, status, remark, thumbnail_path, created_at, updated_at,
              deleted_at, file_size, width, height, convert_format, convert_quality, convert_width,
              annotation_count
       FROM review_image_item WHERE batch_id = ?1 AND deleted_at IS NOT NULL
       ORDER BY deleted_at DESC",
    )?;
    let rows = stmt.query_map([batch_id], map_image_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(ReviewError::from)
  }

  pub fn update_convert_params(
    &self,
    id: i64,
    params: &crate::review::domain::convert_params::ConvertParams,
  ) -> ReviewResult<()> {
    let format = params.format.map(|f| f.extension().to_string());
    self.conn.execute(
      "UPDATE review_image_item SET convert_format = ?1, convert_quality = ?2,
       convert_width = ?3, updated_at = ?4 WHERE id = ?5",
      params![
        format,
        params.quality.map(i64::from),
        params.width.map(|w| w as i64),
        now_ts(),
        id,
      ],
    )?;
    Ok(())
  }

  pub fn update_image_metadata(
    &self,
    id: i64,
    file_size: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
  ) -> ReviewResult<()> {
    self.conn.execute(
      "UPDATE review_image_item SET file_size = ?1, width = ?2, height = ?3, updated_at = ?4 WHERE id = ?5",
      params![
        file_size.map(|v| v as i64),
        width.map(|v| v as i64),
        height.map(|v| v as i64),
        now_ts(),
        id,
      ],
    )?;
    Ok(())
  }
}

impl ReviewRepository for SqliteReviewRepository {
  fn create_batch(&self, name: &str) -> Result<i64, ReviewError> {
    let now = now_ts();
    self.conn.execute(
      "INSERT INTO review_batch (name, total_count, created_at, updated_at)
       VALUES (?1, 0, ?2, ?2)",
      params![name, now],
    )?;
    Ok(self.conn.last_insert_rowid())
  }

  fn list_batches(&self) -> Result<Vec<ReviewBatch>, ReviewError> {
    let mut stmt = self.conn.prepare(
      "SELECT id, name, total_count, created_at, updated_at
       FROM review_batch WHERE deleted_at IS NULL ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], map_batch_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(ReviewError::from)
  }

  fn get_batch(&self, id: i64) -> Result<ReviewBatch, ReviewError> {
    self
      .conn
      .query_row(
        "SELECT id, name, total_count, created_at, updated_at FROM review_batch WHERE id = ?1",
        [id],
        map_batch_row,
      )
      .optional()?
      .ok_or(ReviewError::NotFound {
        entity: "review_batch",
        id,
      })
  }

  fn delete_batch(&self, id: i64) -> Result<(), ReviewError> {
    let tx = self.conn.unchecked_transaction()?;
    let image_ids: Vec<i64> = {
      let mut stmt =
        tx.prepare("SELECT id FROM review_image_item WHERE batch_id = ?1")?;
      let rows = stmt.query_map([id], |row| row.get(0))?;
      rows.collect::<Result<Vec<_>, _>>()?
    };
    for image_id in image_ids {
      tx.execute(
        "DELETE FROM review_annotation WHERE image_item_id = ?1",
        [image_id],
      )?;
    }
    tx.execute("DELETE FROM review_image_item WHERE batch_id = ?1", [id])?;
    let affected = tx.execute("DELETE FROM review_batch WHERE id = ?1", [id])?;
    if affected == 0 {
      return Err(ReviewError::NotFound {
        entity: "review_batch",
        id,
      });
    }
    tx.commit()?;
    Ok(())
  }

  fn add_image_items(
    &self,
    batch_id: i64,
    items: &[NewReviewImageItem],
  ) -> Result<(), ReviewError> {
    if items.is_empty() {
      return Ok(());
    }
    self.get_batch(batch_id)?;
    let tx = self.conn.unchecked_transaction()?;
    let now = now_ts();
    for item in items {
      tx.execute(
        "INSERT INTO review_image_item
         (batch_id, file_path, status, remark, thumbnail_path, created_at, updated_at)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?5)",
        params![
          batch_id,
          item.file_path.to_string_lossy().as_ref(),
          ReviewStatus::Pending.to_sql(),
          item.thumbnail_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
          now,
        ],
      )?;
    }
    tx.execute(
      "UPDATE review_batch
       SET total_count = (SELECT COUNT(*) FROM review_image_item WHERE batch_id = ?1),
           updated_at = ?2
       WHERE id = ?1",
      params![batch_id, now],
    )?;
    tx.commit()?;
    Ok(())
  }

  fn list_image_items(
    &self,
    batch_id: i64,
    filter: ItemFilter,
  ) -> Result<Vec<ReviewImageItem>, ReviewError> {
    let mut sql = String::from(
      "SELECT id, batch_id, file_path, status, remark, thumbnail_path, created_at, updated_at,
              deleted_at, file_size, width, height, convert_format, convert_quality, convert_width,
              annotation_count
       FROM review_image_item WHERE batch_id = ?1 AND deleted_at IS NULL",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(batch_id)];
    if let Some(status) = filter.status {
      sql.push_str(" AND status = ?");
      params_vec.push(Box::new(status.to_sql().to_string()));
    }
    sql.push_str(" ORDER BY file_path ASC");
    let mut stmt = self.conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
      params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), map_image_row)?;
    let mut items: Vec<ReviewImageItem> = rows
      .collect::<Result<Vec<_>, _>>()
      .map_err(ReviewError::from)?;
    let search = filter.search.trim().to_ascii_lowercase();
    if !search.is_empty() {
      items.retain(|i| {
        i.file_path
          .to_string_lossy()
          .to_ascii_lowercase()
          .contains(&search)
      });
    }
    Ok(items)
  }

  fn update_image_status(&self, item_id: i64, status: ReviewStatus) -> Result<(), ReviewError> {
    let now = now_ts();
    let n = self.conn.execute(
      "UPDATE review_image_item SET status = ?1, updated_at = ?2 WHERE id = ?3",
      params![status.to_sql(), now, item_id],
    )?;
    if n == 0 {
      return Err(ReviewError::NotFound {
        entity: "review_image_item",
        id: item_id,
      });
    }
    Ok(())
  }

  fn update_image_remark(&self, item_id: i64, remark: &str) -> Result<(), ReviewError> {
    let now = now_ts();
    self.conn.execute(
      "UPDATE review_image_item SET remark = ?1, updated_at = ?2 WHERE id = ?3",
      params![remark, now, item_id],
    )?;
    Ok(())
  }

  fn list_annotations(&self, image_item_id: i64) -> Result<Vec<Annotation>, ReviewError> {
    let mut stmt = self.conn.prepare(
      "SELECT id, image_item_id, anno_type, position, style, content, created_at, locked, z_index
       FROM review_annotation WHERE image_item_id = ?1 ORDER BY z_index ASC, id ASC",
    )?;
    let rows = stmt.query_map([image_item_id], map_annotation_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(ReviewError::from)
  }

  fn add_annotation(
    &self,
    image_item_id: i64,
    annotation: &Annotation,
  ) -> Result<i64, ReviewError> {
    let pos = serde_json::to_string(&annotation.position)?;
    let style = serde_json::to_string(&annotation.style)?;
    let created_at = annotation.created_at.timestamp();
    self.conn.execute(
      "INSERT INTO review_annotation
       (image_item_id, anno_type, position, style, content, created_at, locked, z_index)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
      params![
        image_item_id,
        annotation.kind.to_sql(),
        pos,
        style,
        if annotation.content.is_empty() {
          None::<String>
        } else {
          Some(annotation.content.clone())
        },
        created_at,
        i32::from(annotation.locked),
        annotation.z_index,
      ],
    )?;
    let id = self.conn.last_insert_rowid();
    let _ = self.conn.execute(
      "UPDATE review_image_item SET annotation_count = annotation_count + 1 WHERE id = ?1",
      [image_item_id],
    );
    Ok(id)
  }

  fn delete_annotation(&self, annotation_id: i64) -> Result<(), ReviewError> {
    let n = self
      .conn
      .execute("DELETE FROM review_annotation WHERE id = ?1", [annotation_id])?;
    if n == 0 {
      return Err(ReviewError::NotFound {
        entity: "review_annotation",
        id: annotation_id,
      });
    }
    Ok(())
  }

  fn clear_annotations(&self, image_item_id: i64) -> Result<(), ReviewError> {
    self.conn.execute(
      "DELETE FROM review_annotation WHERE image_item_id = ?1",
      [image_item_id],
    )?;
    Ok(())
  }

  fn batch_update_status(
    &self,
    item_ids: &[i64],
    status: ReviewStatus,
  ) -> Result<usize, ReviewError> {
    if item_ids.is_empty() {
      return Ok(0);
    }
    let tx = self.conn.unchecked_transaction()?;
    let now = now_ts();
    let mut count = 0usize;
    for id in item_ids {
      let affected = tx.execute(
        "UPDATE review_image_item SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status.to_sql(), now, id],
      )?;
      if affected == 0 {
        return Err(ReviewError::NotFound {
          entity: "review_image_item",
          id: *id,
        });
      }
      count += 1;
    }
    tx.commit()?;
    Ok(count)
  }

  fn batch_add_annotation(
    &self,
    item_ids: &[i64],
    annotation: &Annotation,
  ) -> Result<usize, ReviewError> {
    if item_ids.is_empty() {
      return Ok(0);
    }
    let pos = serde_json::to_string(&annotation.position)?;
    let style = serde_json::to_string(&annotation.style)?;
    let created_at = annotation.created_at.timestamp();
    let tx = self.conn.unchecked_transaction()?;
    let mut count = 0usize;
    for image_id in item_ids {
      let exists: i64 = tx.query_row(
        "SELECT COUNT(*) FROM review_image_item WHERE id = ?1",
        [image_id],
        |row| row.get(0),
      )?;
      if exists == 0 {
        return Err(ReviewError::NotFound {
          entity: "review_image_item",
          id: *image_id,
        });
      }
      tx.execute(
        "INSERT INTO review_annotation
         (image_item_id, anno_type, position, style, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
          image_id,
          annotation.kind.to_sql(),
          pos,
          style,
          if annotation.content.is_empty() {
            None::<String>
          } else {
            Some(annotation.content.clone())
          },
          created_at,
        ],
      )?;
      count += 1;
    }
    tx.commit()?;
    Ok(count)
  }
}

impl crate::review::storage::traits::ReviewStorage for SqliteReviewRepository {
  fn get_image(&self, id: i64) -> ReviewResult<ReviewImageItem> {
    SqliteReviewRepository::get_image(self, id)
  }

  fn get_images_by_ids(&self, ids: &[i64]) -> ReviewResult<Vec<ReviewImageItem>> {
    SqliteReviewRepository::get_images_by_ids(self, ids)
  }

  fn list_images(&self, batch_id: i64, filter: &ImageFilter) -> ReviewResult<Vec<ReviewImageItem>> {
    SqliteReviewRepository::list_images(self, batch_id, filter)
  }

  fn batch_set_status(&self, ids: &[i64], status: ReviewStatus) -> ReviewResult<()> {
    SqliteReviewRepository::batch_set_status(self, ids, status)
  }

  fn batch_set_remarks(
    &self,
    ids: &[i64],
    text: &str,
    mode: crate::review::storage::traits::RemarkWriteMode,
  ) -> ReviewResult<()> {
    SqliteReviewRepository::batch_set_remarks(self, ids, text, mode)
  }

  fn batch_insert_annotation_template(
    &self,
    template: &crate::review::storage::traits::AnnotationTemplate,
    image_ids: &[i64],
  ) -> ReviewResult<Vec<i64>> {
    SqliteReviewRepository::batch_insert_annotation_template(self, template, image_ids)
  }

  fn delete_annotations_by_ids(&self, annotation_ids: &[i64]) -> ReviewResult<()> {
    SqliteReviewRepository::delete_annotations_by_ids(self, annotation_ids)
  }

  fn list_annotations(&self, image_item_id: i64) -> ReviewResult<Vec<Annotation>> {
    ReviewRepository::list_annotations(self, image_item_id)
  }

  fn list_export_rows(
    &self,
    query: &crate::review::storage::traits::ExportRowsQuery,
  ) -> ReviewResult<Vec<crate::review::storage::traits::ReviewExportRow>> {
    SqliteReviewRepository::list_export_rows(self, query)
  }
}

fn now_ts() -> i64 {
  Utc::now().timestamp()
}

fn ts_to_dt(ts: i64) -> DateTime<Utc> {
  DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now)
}

fn map_batch_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewBatch> {
  Ok(ReviewBatch {
    id: row.get(0)?,
    name: row.get(1)?,
    total_count: row.get(2)?,
    created_at: ts_to_dt(row.get(3)?),
    updated_at: ts_to_dt(row.get(4)?),
  })
}

fn map_image_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewImageItem> {
  use crate::core::types::ImageFormat;
  use crate::review::domain::convert_params::ConvertParams;

  let status_raw: String = row.get(3)?;
  let convert_format: Option<String> = row.get(12)?;
  let convert_quality: Option<i32> = row.get(13)?;
  let convert_width: Option<i32> = row.get(14)?;
  let format = convert_format.and_then(|s| ImageFormat::from_extension(&s.to_ascii_lowercase()));
  let convert_params = ConvertParams {
    format,
    quality: convert_quality.map(|q| q.clamp(1, 100) as u8),
    width: convert_width.map(|w| w.max(0) as u32).filter(|&w| w > 0),
  };
  Ok(ReviewImageItem {
    id: row.get(0)?,
    batch_id: row.get(1)?,
    file_path: PathBuf::from(row.get::<_, String>(2)?),
    status: ReviewStatus::from_sql(&status_raw).unwrap_or(ReviewStatus::Pending),
    remark: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
    thumbnail_path: row.get::<_, Option<String>>(5)?.map(PathBuf::from),
    created_at: ts_to_dt(row.get(6)?),
    updated_at: ts_to_dt(row.get(7)?),
    deleted_at: row
      .get::<_, Option<i64>>(8)?
      .and_then(|ts| DateTime::from_timestamp(ts, 0)),
    file_size: row.get::<_, Option<i64>>(9)?.map(|v| v.max(0) as u64),
    width: row.get::<_, Option<i32>>(10)?.map(|v| v.max(0) as u32),
    height: row.get::<_, Option<i32>>(11)?.map(|v| v.max(0) as u32),
    convert_params,
    annotation_count: row.get::<_, Option<i32>>(15)?.unwrap_or(0),
  })
}

fn map_annotation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Annotation> {
  let kind_raw: String = row.get(2)?;
  let pos_str: String = row.get(3)?;
  let style_str: String = row.get(4)?;
  let kind = AnnotationKind::from_sql(&kind_raw).unwrap_or(AnnotationKind::Rectangle);
  let position: AnnotationPosition = serde_json::from_str(&pos_str)
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
  let style: AnnotationStyle = serde_json::from_str(&style_str)
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
  let locked: i32 = row.get::<_, Option<i32>>(7)?.unwrap_or(0);
  let z_index: i32 = row.get::<_, Option<i32>>(8)?.unwrap_or(0);
  Ok(Annotation {
    id: row.get(0)?,
    image_item_id: row.get(1)?,
    kind,
    position,
    style,
    content: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
    created_at: ts_to_dt(row.get(6)?),
    locked: locked != 0,
    z_index,
  })
}

pub fn ensure_cache_dirs() -> ReviewResult<()> {
  let dir = crate::review::storage::thumbnail_cache_dir()?;
  std::fs::create_dir_all(dir)?;
  std::fs::create_dir_all(app_data_dir()?)?;
  Ok(())
}

#[allow(dead_code)]
fn with_tx<F>(conn: &Connection, f: F) -> ReviewResult<()>
where
  F: FnOnce(&Transaction<'_>) -> ReviewResult<()>,
{
  let tx = conn.unchecked_transaction()?;
  f(&tx)?;
  tx.commit()?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::review::domain::annotation::{
    AnnotationKind, AnnotationPosition, AnnotationStyle, RectanglePosition,
  };

  fn repo() -> SqliteReviewRepository {
    SqliteReviewRepository::open_memory().unwrap()
  }

  #[test]
  fn trait_create_batch_and_items() {
    let repo = repo();
    let batch_id = repo.create_batch("测试批次", &[]).unwrap();
    assert_eq!(batch_id, 1);
    repo
      .add_image_items(
        batch_id,
        &[NewReviewImageItem {
          file_path: PathBuf::from("/a/1.jpg"),
          thumbnail_path: None,
        }],
      )
      .unwrap();
    let batch = repo.get_batch(batch_id).unwrap();
    assert_eq!(batch.total_count, 1);
    let items = repo
      .list_image_items(batch_id, ItemFilter::default())
      .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].status, ReviewStatus::Pending);
  }

  #[test]
  fn trait_batch_update_status_returns_count() {
    let repo = repo();
    let batch_id = repo.create_batch("b", &[PathBuf::from("/x.jpg")]).unwrap();
    let ids: Vec<i64> = repo
      .list_image_items(batch_id, ItemFilter::default())
      .unwrap()
      .into_iter()
      .map(|i| i.id)
      .collect();
    let n = repo
      .batch_update_status(&ids, ReviewStatus::Approved)
      .unwrap();
    assert_eq!(n, 1);
    let item = repo.get_image(ids[0]).unwrap();
    assert_eq!(item.status, ReviewStatus::Approved);
  }

  #[test]
  fn trait_annotations_roundtrip() {
    let repo = repo();
    let batch_id = repo.create_batch("b", &[PathBuf::from("/x.jpg")]).unwrap();
    let image_id = repo
      .list_image_items(batch_id, ItemFilter::default())
      .unwrap()[0]
      .id;
    let ann = Annotation::new_draft(
      image_id,
      AnnotationKind::Rectangle,
      AnnotationPosition::Rectangle(RectanglePosition {
        x0: 0.1,
        y0: 0.2,
        x1: 0.5,
        y1: 0.6,
      }),
      AnnotationStyle::default(),
      String::new(),
    );
    let ann_id = repo.add_annotation(image_id, &ann).unwrap();
    let list = repo.list_annotations(image_id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, ann_id);
    repo.clear_annotations(image_id).unwrap();
    assert!(repo.list_annotations(image_id).unwrap().is_empty());
  }

  #[test]
  fn shared_connection_init() {
    let conn = Connection::open_in_memory().unwrap();
    SqliteReviewRepository::init(&conn).unwrap();
    let repo = SqliteReviewRepository::new(conn).unwrap();
    let id = ReviewRepository::create_batch(&repo, "shared").unwrap();
    assert_eq!(id, 1);
  }
}
