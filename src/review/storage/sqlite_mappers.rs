//! SQLite 行映射与时间/颜色辅助。

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::review::domain::annotation::{
    Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle,
};
use crate::review::domain::batch::ReviewBatch;
use crate::review::domain::image_item::{ReviewImageItem, ReviewStatus};

pub(crate) fn now_ts() -> i64 {
    Utc::now().timestamp()
}

pub(crate) fn ts_to_dt(ts: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now)
}

pub(crate) fn color_to_hex(color: [u8; 4]) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color[0], color[1], color[2], color[3]
    )
}

pub(crate) fn hex_to_color(hex: &str) -> [u8; 4] {
    let s = hex.trim_start_matches('#');
    let parse = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    match s.len() {
        8 => [parse(0), parse(2), parse(4), parse(6)],
        6 => [parse(0), parse(2), parse(4), 255],
        _ => [142, 142, 147, 255],
    }
}

pub(crate) fn map_tag_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<crate::review::domain::ReviewTag> {
    let color_hex: String = row.get(2)?;
    Ok(crate::review::domain::ReviewTag {
        id: row.get(0)?,
        name: row.get(1)?,
        color: hex_to_color(&color_hex),
        created_at: ts_to_dt(row.get(3)?),
    })
}

pub(crate) fn map_batch_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewBatch> {
    Ok(ReviewBatch {
        id: row.get(0)?,
        name: row.get(1)?,
        total_count: row.get(2)?,
        created_at: ts_to_dt(row.get(3)?),
        updated_at: ts_to_dt(row.get(4)?),
    })
}

pub(crate) fn map_image_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewImageItem> {
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
        jira_issue_key: row.get::<_, Option<String>>(16)?,
        jira_url: row.get::<_, Option<String>>(17)?,
    })
}

pub(crate) fn map_annotation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Annotation> {
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
