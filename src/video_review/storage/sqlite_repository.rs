//! SQLite 视频评审仓储实现。

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::review::domain::image_item::ReviewStatus;
use crate::review::storage::paths::database_path;
use crate::video_review::domain::{
    BatchStats, MarkerKind, VideoBatch, VideoDefect, VideoFilter, VideoItem, VideoMarker,
    VideoSegment, VideoTag,
};
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::storage::migrate;
use crate::video_review::storage::repository::{NewVideoItem, VideoRepository};

pub struct SqliteVideoRepository {
    conn: Connection,
}

impl SqliteVideoRepository {
    pub fn new(conn: Connection) -> VideoReviewResult<Self> {
        migrate::ensure_schema(&conn)?;
        Ok(Self { conn })
    }

    pub fn open() -> VideoReviewResult<Self> {
        let path = database_path().map_err(|e| VideoReviewError::Message(e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::new(conn)
    }

    pub fn open_memory() -> VideoReviewResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::new(conn)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn create_batch_with_videos(
        &self,
        name: &str,
        items: &[NewVideoItem],
    ) -> VideoReviewResult<i64> {
        let batch_id = self.create_batch(name)?;
        if !items.is_empty() {
            self.add_videos(batch_id, items)?;
        }
        Ok(batch_id)
    }
}

fn now_ts() -> i64 {
    Utc::now().timestamp()
}

fn color_to_sql(c: [u8; 4]) -> String {
    format!("{},{},{},{}", c[0], c[1], c[2], c[3])
}

fn color_from_sql(raw: &str) -> [u8; 4] {
    let parts: Vec<u8> = raw
        .split(',')
        .filter_map(|p| p.trim().parse::<u8>().ok())
        .collect();
    if parts.len() == 4 {
        [parts[0], parts[1], parts[2], parts[3]]
    } else {
        [142, 142, 147, 255]
    }
}

fn map_batch_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VideoBatch> {
    Ok(VideoBatch {
        id: row.get(0)?,
        name: row.get(1)?,
        total_count: row.get(2)?,
        created_at: ts_to_dt(row.get(3)?),
        updated_at: ts_to_dt(row.get(4)?),
    })
}

fn map_video_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VideoItem> {
    let thumb: Option<String> = row.get(5)?;
    Ok(VideoItem {
        id: row.get(0)?,
        batch_id: row.get(1)?,
        file_path: PathBuf::from(row.get::<_, String>(2)?),
        status: ReviewStatus::from_sql(&row.get::<_, String>(3)?).unwrap_or(ReviewStatus::Pending),
        remark: row.get(4)?,
        thumbnail_path: thumb.map(PathBuf::from),
        duration_ms: row.get::<_, i64>(6)? as u64,
        fps: row.get(7)?,
        width: row.get(8)?,
        height: row.get(9)?,
        video_codec: row.get(10)?,
        audio_codec: row.get(11)?,
        bitrate_kbps: row.get(12)?,
        device_model: row.get(13)?,
        offset_ms: row.get(14)?,
        created_at: ts_to_dt(row.get(15)?),
        updated_at: ts_to_dt(row.get(16)?),
        deleted_at: row.get::<_, Option<i64>>(17)?.map(ts_to_dt),
    })
}

fn ts_to_dt(ts: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now)
}

impl VideoRepository for SqliteVideoRepository {
    fn create_batch(&self, name: &str) -> VideoReviewResult<i64> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO video_review_batch (name, total_count, created_at, updated_at)
       VALUES (?1, 0, ?2, ?2)",
            params![name, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn list_batches(&self) -> VideoReviewResult<Vec<VideoBatch>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, total_count, created_at, updated_at
       FROM video_review_batch ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], map_batch_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn get_batch(&self, id: i64) -> VideoReviewResult<VideoBatch> {
        self
      .conn
      .query_row(
        "SELECT id, name, total_count, created_at, updated_at FROM video_review_batch WHERE id = ?1",
        [id],
        map_batch_row,
      )
      .optional()?
      .ok_or(VideoReviewError::NotFound {
        entity: "video_review_batch",
        id,
      })
    }

    fn delete_batch(&self, id: i64) -> VideoReviewResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM video_review_marker WHERE video_id IN (SELECT id FROM video_review_item WHERE batch_id = ?1)", [id])?;
        tx.execute("DELETE FROM video_review_segment WHERE video_id IN (SELECT id FROM video_review_item WHERE batch_id = ?1)", [id])?;
        tx.execute("DELETE FROM video_review_item_tag WHERE video_id IN (SELECT id FROM video_review_item WHERE batch_id = ?1)", [id])?;
        tx.execute("DELETE FROM video_review_item WHERE batch_id = ?1", [id])?;
        tx.execute("DELETE FROM video_review_batch WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    fn add_videos(&self, batch_id: i64, items: &[NewVideoItem]) -> VideoReviewResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        let now = now_ts();
        for item in items {
            tx.execute(
                "INSERT OR IGNORE INTO video_review_item
         (batch_id, file_path, status, thumbnail_path, duration_ms, fps, width, height,
          video_codec, audio_codec, bitrate_kbps, device_model, offset_ms, created_at, updated_at)
         VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?12)",
                params![
                    batch_id,
                    item.file_path.to_string_lossy().as_ref(),
                    item.thumbnail_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    item.duration_ms as i64,
                    item.fps,
                    item.width,
                    item.height,
                    item.video_codec,
                    item.audio_codec,
                    item.bitrate_kbps,
                    item.device_model,
                    now,
                ],
            )?;
        }
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM video_review_item WHERE batch_id = ?1 AND deleted_at IS NULL",
            [batch_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE video_review_batch SET total_count = ?1, updated_at = ?2 WHERE id = ?3",
            params![count, now, batch_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn list_videos(
        &self,
        batch_id: i64,
        filter: &VideoFilter,
    ) -> VideoReviewResult<Vec<VideoItem>> {
        let mut sql = String::from(
      "SELECT id, batch_id, file_path, status, remark, thumbnail_path, duration_ms, fps, width, height,
              video_codec, audio_codec, bitrate_kbps, device_model, offset_ms, created_at, updated_at, deleted_at
       FROM video_review_item WHERE batch_id = ?1",
    );
        if !filter.include_deleted {
            sql.push_str(" AND deleted_at IS NULL");
        }
        if let Some(status) = filter.status {
            sql.push_str(&format!(" AND status = '{}'", status.to_sql()));
        }
        if !filter.tag_ids.is_empty() {
            sql.push_str(
                " AND id IN (SELECT video_id FROM video_review_item_tag WHERE tag_id IN (",
            );
            for (i, tid) in filter.tag_ids.iter().enumerate() {
                if i > 0 {
                    sql.push(',');
                }
                sql.push_str(&tid.to_string());
            }
            sql.push(')');
        }
        sql.push_str(" ORDER BY file_path ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([batch_id], map_video_row)?;
        let mut items: Vec<VideoItem> = rows.collect::<Result<Vec<_>, _>>()?;
        filter.apply_in_memory(&mut items);
        Ok(items)
    }

    fn get_video(&self, id: i64) -> VideoReviewResult<VideoItem> {
        self
      .conn
      .query_row(
        "SELECT id, batch_id, file_path, status, remark, thumbnail_path, duration_ms, fps, width, height,
                video_codec, audio_codec, bitrate_kbps, device_model, offset_ms, created_at, updated_at, deleted_at
         FROM video_review_item WHERE id = ?1",
        [id],
        map_video_row,
      )
      .optional()?
      .ok_or(VideoReviewError::NotFound {
        entity: "video_review_item",
        id,
      })
    }

    fn update_video_status(&self, id: i64, status: ReviewStatus) -> VideoReviewResult<()> {
        self.conn.execute(
            "UPDATE video_review_item SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_sql(), now_ts(), id],
        )?;
        Ok(())
    }

    fn update_video_remark(&self, id: i64, remark: &str) -> VideoReviewResult<()> {
        self.conn.execute(
            "UPDATE video_review_item SET remark = ?1, updated_at = ?2 WHERE id = ?3",
            params![remark, now_ts(), id],
        )?;
        Ok(())
    }

    fn update_video_device_model(
        &self,
        id: i64,
        device_model: Option<&str>,
    ) -> VideoReviewResult<()> {
        self.conn.execute(
            "UPDATE video_review_item SET device_model = ?1, updated_at = ?2 WHERE id = ?3",
            params![device_model, now_ts(), id],
        )?;
        Ok(())
    }

    fn update_video_offset(&self, id: i64, offset_ms: i64) -> VideoReviewResult<()> {
        self.conn.execute(
            "UPDATE video_review_item SET offset_ms = ?1, updated_at = ?2 WHERE id = ?3",
            params![offset_ms, now_ts(), id],
        )?;
        Ok(())
    }

    fn set_thumbnail_path(&self, id: i64, path: &Path) -> VideoReviewResult<()> {
        self.conn.execute(
            "UPDATE video_review_item SET thumbnail_path = ?1, updated_at = ?2 WHERE id = ?3",
            params![path.to_string_lossy().as_ref(), now_ts(), id],
        )?;
        Ok(())
    }

    fn batch_stats(&self, batch_id: i64) -> VideoReviewResult<BatchStats> {
        let mut stmt = self.conn.prepare(
            "SELECT status, COUNT(*) FROM video_review_item
       WHERE batch_id = ?1 AND deleted_at IS NULL GROUP BY status",
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

    fn list_tags(&self) -> VideoReviewResult<Vec<VideoTag>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, color, created_at FROM video_review_tag ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(VideoTag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: color_from_sql(&row.get::<_, String>(2)?),
                created_at: ts_to_dt(row.get(3)?),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn create_tag(&self, name: &str, color: [u8; 4]) -> VideoReviewResult<i64> {
        self.conn.execute(
            "INSERT INTO video_review_tag (name, color, created_at) VALUES (?1, ?2, ?3)",
            params![name, color_to_sql(color), now_ts()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn delete_tag(&self, id: i64) -> VideoReviewResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM video_review_item_tag WHERE tag_id = ?1", [id])?;
        tx.execute("DELETE FROM video_review_tag WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    fn get_video_tag_ids(&self, video_id: i64) -> VideoReviewResult<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag_id FROM video_review_item_tag WHERE video_id = ?1")?;
        let rows = stmt.query_map([video_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn set_video_tags(&self, video_id: i64, tag_ids: &[i64]) -> VideoReviewResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM video_review_item_tag WHERE video_id = ?1",
            [video_id],
        )?;
        for tid in tag_ids {
            tx.execute(
                "INSERT INTO video_review_item_tag (video_id, tag_id) VALUES (?1, ?2)",
                params![video_id, tid],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn batch_set_tags(&self, video_ids: &[i64], tag_ids: &[i64]) -> VideoReviewResult<()> {
        for vid in video_ids {
            self.set_video_tags(*vid, tag_ids)?;
        }
        Ok(())
    }

    fn batch_update_status(&self, ids: &[i64], status: ReviewStatus) -> VideoReviewResult<()> {
        let now = now_ts();
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            tx.execute(
                "UPDATE video_review_item SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![status.to_sql(), now, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn batch_append_remark(&self, ids: &[i64], text: &str) -> VideoReviewResult<()> {
        if text.is_empty() {
            return Ok(());
        }
        let now = now_ts();
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            let current: Option<String> = tx.query_row(
                "SELECT remark FROM video_review_item WHERE id = ?1",
                [id],
                |row| row.get(0),
            )?;
            let new_remark = match current {
                None => text.to_string(),
                Some(ref s) if s.is_empty() => text.to_string(),
                Some(current) => format!("{current}\n{text}"),
            };
            tx.execute(
                "UPDATE video_review_item SET remark = ?1, updated_at = ?2 WHERE id = ?3",
                params![new_remark, now, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn add_marker(
        &self,
        video_id: i64,
        time_ms: u64,
        kind: MarkerKind,
        text: &str,
        severity: u8,
    ) -> VideoReviewResult<i64> {
        self.conn.execute(
            "INSERT INTO video_review_marker (video_id, time_ms, kind, text, severity, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                video_id,
                time_ms as i64,
                kind.to_sql(),
                text,
                severity,
                now_ts()
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn list_markers(&self, video_id: i64) -> VideoReviewResult<Vec<VideoMarker>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, video_id, time_ms, kind, text, severity, created_at
       FROM video_review_marker WHERE video_id = ?1 ORDER BY time_ms ASC",
        )?;
        let rows = stmt.query_map([video_id], |row| {
            Ok(VideoMarker {
                id: row.get(0)?,
                video_id: row.get(1)?,
                time_ms: row.get::<_, i64>(2)? as u64,
                kind: MarkerKind::from_sql(&row.get::<_, String>(3)?).unwrap_or(MarkerKind::Note),
                text: row.get(4)?,
                severity: row.get(5)?,
                created_at: ts_to_dt(row.get(6)?),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn delete_marker(&self, id: i64) -> VideoReviewResult<()> {
        self.conn
            .execute("DELETE FROM video_review_marker WHERE id = ?1", [id])?;
        Ok(())
    }

    fn add_segment(
        &self,
        video_id: i64,
        start_ms: u64,
        end_ms: u64,
        text: &str,
        status: ReviewStatus,
    ) -> VideoReviewResult<i64> {
        self.conn.execute(
      "INSERT INTO video_review_segment (video_id, start_ms, end_ms, text, status, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
      params![
        video_id,
        start_ms as i64,
        end_ms as i64,
        text,
        status.to_sql(),
        now_ts()
      ],
    )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn list_segments(&self, video_id: i64) -> VideoReviewResult<Vec<VideoSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, video_id, start_ms, end_ms, text, status, created_at
       FROM video_review_segment WHERE video_id = ?1 ORDER BY start_ms ASC",
        )?;
        let rows = stmt.query_map([video_id], |row| {
            Ok(VideoSegment {
                id: row.get(0)?,
                video_id: row.get(1)?,
                start_ms: row.get::<_, i64>(2)? as u64,
                end_ms: row.get::<_, i64>(3)? as u64,
                text: row.get(4)?,
                status: ReviewStatus::from_sql(&row.get::<_, String>(5)?)
                    .unwrap_or(ReviewStatus::Pending),
                created_at: ts_to_dt(row.get(6)?),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn delete_segment(&self, id: i64) -> VideoReviewResult<()> {
        self.conn
            .execute("DELETE FROM video_review_segment WHERE id = ?1", [id])?;
        Ok(())
    }

    fn save_session_value(&self, key: &str, value: &str) -> VideoReviewResult<()> {
        self.conn.execute(
            "INSERT INTO video_review_compare_session (key, value) VALUES (?1, ?2)
       ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn load_session_value(&self, key: &str) -> VideoReviewResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM video_review_compare_session WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    fn create_defect(
        &self,
        batch_id: i64,
        title: &str,
        description: &str,
        severity: u8,
        time_ms: u64,
        half_window_ms: u64,
        video_ids: &[i64],
        package_path: Option<&Path>,
    ) -> VideoReviewResult<VideoDefect> {
        let ids_json = serde_json::to_string(video_ids)?;
        let path_str = package_path.map(|p| p.to_string_lossy().to_string());
        let created = now_ts();
        self.conn.execute(
            "INSERT INTO video_review_defect
             (batch_id, title, description, severity, time_ms, half_window_ms, video_ids, package_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                batch_id,
                title,
                description,
                severity as i64,
                time_ms as i64,
                half_window_ms as i64,
                ids_json,
                path_str,
                created,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(VideoDefect {
            id,
            batch_id,
            title: title.to_string(),
            description: description.to_string(),
            severity,
            time_ms,
            half_window_ms,
            video_ids: video_ids.to_vec(),
            package_path: package_path.map(Path::to_path_buf),
            created_at: DateTime::from_timestamp(created, 0).unwrap_or_else(Utc::now),
        })
    }

    fn list_defects(&self, batch_id: i64) -> VideoReviewResult<Vec<VideoDefect>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, batch_id, title, description, severity, time_ms, half_window_ms,
                    video_ids, package_path, created_at
             FROM video_review_defect WHERE batch_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([batch_id], |row| {
            let ids_json: String = row.get(7)?;
            let video_ids: Vec<i64> = serde_json::from_str(&ids_json).unwrap_or_default();
            let package_path: Option<String> = row.get(8)?;
            let created: i64 = row.get(9)?;
            Ok(VideoDefect {
                id: row.get(0)?,
                batch_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                severity: row.get::<_, i64>(4)? as u8,
                time_ms: row.get::<_, i64>(5)? as u64,
                half_window_ms: row.get::<_, i64>(6)? as u64,
                video_ids,
                package_path: package_path.map(PathBuf::from),
                created_at: DateTime::from_timestamp(created, 0).unwrap_or_else(Utc::now),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_review::domain::MarkerKind;

    fn sample_item(path: &str) -> NewVideoItem {
        NewVideoItem {
            file_path: PathBuf::from(path),
            thumbnail_path: None,
            duration_ms: 60_000,
            fps: 24.0,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            audio_codec: Some("aac".into()),
            bitrate_kbps: Some(5000),
            device_model: Some("Pixel 8".into()),
        }
    }

    #[test]
    fn crud_batch_and_video() {
        let repo = SqliteVideoRepository::open_memory().unwrap();
        let batch_id = repo
            .create_batch_with_videos("test", &[sample_item("/tmp/a.mp4")])
            .unwrap();
        let videos = repo.list_videos(batch_id, &VideoFilter::default()).unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].device_model.as_deref(), Some("Pixel 8"));
        repo.update_video_device_model(videos[0].id, Some("iPhone 15"))
            .unwrap();
        assert_eq!(
            repo.get_video(videos[0].id)
                .unwrap()
                .device_model
                .as_deref(),
            Some("iPhone 15")
        );
        repo.update_video_status(videos[0].id, ReviewStatus::Approved)
            .unwrap();
        let stats = repo.batch_stats(batch_id).unwrap();
        assert_eq!(stats.approved, 1);
    }

    #[test]
    fn markers_and_segments() {
        let repo = SqliteVideoRepository::open_memory().unwrap();
        let batch_id = repo
            .create_batch_with_videos("m", &[sample_item("/tmp/b.mp4")])
            .unwrap();
        let vid = repo.list_videos(batch_id, &VideoFilter::default()).unwrap()[0].id;
        repo.add_marker(vid, 1000, MarkerKind::Issue, "抖动", 2)
            .unwrap();
        repo.add_segment(vid, 0, 5000, "片头", ReviewStatus::NeedsFix)
            .unwrap();
        assert_eq!(repo.list_markers(vid).unwrap().len(), 1);
        assert_eq!(repo.list_segments(vid).unwrap().len(), 1);
    }

    #[test]
    fn batch_status_and_remark() {
        let repo = SqliteVideoRepository::open_memory().unwrap();
        let batch_id = repo
            .create_batch_with_videos("m", &[sample_item("/tmp/b.mp4"), sample_item("/tmp/c.mp4")])
            .unwrap();
        let videos = repo.list_videos(batch_id, &VideoFilter::default()).unwrap();
        let ids: Vec<i64> = videos.iter().map(|v| v.id).collect();
        repo.batch_update_status(&ids, ReviewStatus::Approved)
            .unwrap();
        repo.batch_append_remark(&ids, "批量备注").unwrap();
        let v = repo.get_video(ids[0]).unwrap();
        assert_eq!(v.status, ReviewStatus::Approved);
        assert_eq!(v.remark.as_deref(), Some("批量备注"));
    }
}
