//! 评审数据导出：CSV（UTF-8 BOM）与标注 JSON 侧载。

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::review::domain::annotation::Annotation;
use crate::review::domain::image_item::{ImageFilter, ReviewStatus};
use crate::review::error::ReviewResult;
use crate::review::storage::traits::{ExportRowsQuery, ReviewExportRow, ReviewStorage};

/// CSV 导出请求。
#[derive(Debug, Clone)]
pub struct CsvExportRequest {
  pub batch_id: i64,
  pub dest: PathBuf,
  /// 按状态筛选；`None` 表示不过滤。
  pub status_filter: Option<ReviewStatus>,
  /// 仅导出指定图片项；`None` 表示导出批次内全部（可叠加状态筛选）。
  pub image_ids: Option<Vec<i64>>,
}

/// CSV 导出结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvExportResult {
  pub row_count: usize,
  pub dest: PathBuf,
}

/// 单图 JSON 侧载导出请求。
#[derive(Debug, Clone)]
pub struct JsonSidecarRequest {
  pub image_item_id: i64,
  pub image_path: PathBuf,
  /// 若指定则写入该目录，否则与图片同目录。
  pub output_dir: Option<PathBuf>,
}

/// 批次 JSON 侧载导出请求。
#[derive(Debug, Clone)]
pub struct BatchJsonExportRequest {
  pub batch_id: i64,
  pub output_dir: PathBuf,
}

/// 导出服务（无 UI 依赖）。
pub struct ExportService;

impl ExportService {
  /// 导出评审结果为 CSV（UTF-8 with BOM，Excel 友好）。
  pub fn export_csv<S: ReviewStorage + ?Sized>(
    storage: &S,
    request: &CsvExportRequest,
  ) -> ReviewResult<CsvExportResult> {
    let rows = storage.list_export_rows(&ExportRowsQuery {
      batch_id: request.batch_id,
      status_filter: request.status_filter,
      image_ids: request.image_ids.clone(),
    })?;
    write_csv_with_bom(&request.dest, &rows)?;
    Ok(CsvExportResult {
      row_count: rows.len(),
      dest: request.dest.clone(),
    })
  }

  /// 导出单图标注 JSON 侧载文件。
  pub fn export_annotation_json<S: ReviewStorage + ?Sized>(
    storage: &S,
    request: &JsonSidecarRequest,
  ) -> ReviewResult<PathBuf> {
    let annotations = storage.list_annotations(request.image_item_id)?;
    let dest = sidecar_path(&request.image_path, request.output_dir.as_deref());
    write_annotation_json(&dest, &request.image_path, &annotations)?;
    Ok(dest)
  }

  /// 批量导出批次内所有图片的标注 JSON。
  pub fn export_batch_annotation_json<S: ReviewStorage + ?Sized>(
    storage: &S,
    request: &BatchJsonExportRequest,
  ) -> ReviewResult<Vec<PathBuf>> {
    std::fs::create_dir_all(&request.output_dir)?;
    let images = storage.list_images(request.batch_id, &ImageFilter::default())?;
    let mut written = Vec::with_capacity(images.len());
    for item in images {
      let annotations = storage.list_annotations(item.id)?;
      let file_name = item
        .file_path
        .file_stem()
        .map(|s| format!("{}.json", s.to_string_lossy()))
        .unwrap_or_else(|| format!("image_{}.json", item.id));
      let dest = request.output_dir.join(file_name);
      write_annotation_json(&dest, &item.file_path, &annotations)?;
      written.push(dest);
    }
    Ok(written)
  }

  /// 兼容旧接口：导出整批 CSV。
  pub fn export_batch_csv_legacy<S: ReviewStorage + ?Sized>(
    storage: &S,
    batch_id: i64,
    dest: &Path,
  ) -> ReviewResult<()> {
    Self::export_csv(
      storage,
      &CsvExportRequest {
        batch_id,
        dest: dest.to_path_buf(),
        status_filter: None,
        image_ids: None,
      },
    )?;
    Ok(())
  }
}

fn write_csv_with_bom(dest: &Path, rows: &[ReviewExportRow]) -> ReviewResult<()> {
  let mut file = File::create(dest)?;
  file.write_all("\u{feff}".as_bytes())?;
  {
    let mut wtr = csv::Writer::from_writer(&mut file);
    wtr.write_record([
      "文件名",
      "文件路径",
      "评审状态",
      "备注",
      "标注数量",
      "最后评审时间",
    ])?;
    for row in rows {
      wtr.write_record([
        &row.file_name,
        &row.file_path,
        row.status.label(),
        &row.remark,
        &row.annotation_count.to_string(),
        &row.updated_at,
      ])?;
    }
    wtr.flush()?;
  }
  file.flush()?;
  Ok(())
}

fn sidecar_path(image_path: &Path, output_dir: Option<&Path>) -> PathBuf {
  let file_name = image_path
    .file_stem()
    .map(|s| format!("{}.json", s.to_string_lossy()))
    .unwrap_or_else(|| "annotations.json".into());
  match output_dir {
    Some(dir) => dir.join(file_name),
    None => image_path.with_extension("json"),
  }
}

fn write_annotation_json(
  dest: &Path,
  image_path: &Path,
  annotations: &[Annotation],
) -> ReviewResult<()> {
  if let Some(parent) = dest.parent() {
    std::fs::create_dir_all(parent)?;
  }
  let payload = serde_json::json!({
    "image": image_path.to_string_lossy(),
    "annotations": annotations.iter().map(annotation_to_json).collect::<Vec<_>>(),
  });
  std::fs::write(dest, serde_json::to_string_pretty(&payload)?)?;
  Ok(())
}

fn annotation_to_json(ann: &Annotation) -> serde_json::Value {
  serde_json::json!({
    "id": ann.id,
    "type": ann.kind.db_value(),
    "kind": format!("{:?}", ann.kind),
    "position": ann.position,
    "style": ann.style,
    "content": ann.content,
    "created_at": ann.created_at.to_rfc3339(),
  })
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use crate::review::domain::annotation::{
    Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle, TextPosition,
  };
  use crate::review::domain::image_item::ReviewStatus;
  use crate::review::storage::SqliteReviewRepository;
  use super::*;

  fn fixture() -> (SqliteReviewRepository, i64, Vec<i64>) {
    let repo = SqliteReviewRepository::open_memory().unwrap();
    let paths = vec![PathBuf::from("/data/photo.jpg")];
    let batch_id = repo.create_batch("b", &paths).unwrap();
    let ids: Vec<i64> = repo
      .list_images(batch_id, &ImageFilter::default())
      .unwrap()
      .into_iter()
      .map(|i| i.id)
      .collect();
    repo.batch_set_status(&ids, ReviewStatus::Approved).unwrap();
    (repo, batch_id, ids)
  }

  #[test]
  fn csv_export_has_bom_and_chinese_headers() {
    let (repo, batch_id, _) = fixture();
    let dest = std::env::temp_dir().join("imgforge_review_export_test.csv");
    let result = ExportService::export_csv(
      &repo,
      &CsvExportRequest {
        batch_id,
        dest: dest.clone(),
        status_filter: None,
        image_ids: None,
      },
    )
    .unwrap();
    assert_eq!(result.row_count, 1);
    let bytes = std::fs::read(&dest).unwrap();
    assert_eq!(&bytes[0..3], [0xEF, 0xBB, 0xBF]);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("文件名"));
    assert!(text.contains("通过"));
    let _ = std::fs::remove_file(dest);
  }

  #[test]
  fn csv_export_filters_by_status() {
    let (repo, batch_id, ids) = fixture();
    repo.batch_set_status(&[ids[0]], ReviewStatus::Rejected).unwrap();
    let dest = std::env::temp_dir().join("imgforge_review_filter.csv");
    ExportService::export_csv(
      &repo,
      &CsvExportRequest {
        batch_id,
        dest: dest.clone(),
        status_filter: Some(ReviewStatus::Approved),
        image_ids: None,
      },
    )
    .unwrap();
    let text = String::from_utf8(std::fs::read(&dest).unwrap()).unwrap();
    assert!(!text.contains("驳回"));
    let _ = std::fs::remove_file(dest);
  }

  #[test]
  fn json_sidecar_round_trip() {
    let (repo, _batch_id, ids) = fixture();
    let ann = Annotation::new_draft(
      ids[0],
      AnnotationKind::Text,
      AnnotationPosition::Text(TextPosition { x: 0.5, y: 0.5 }),
      AnnotationStyle::default(),
      "备注".into(),
    );
    repo.insert_annotation(&ann).unwrap();

    let out_dir = std::env::temp_dir().join("imgforge_json_export");
    let path = ExportService::export_annotation_json(
      &repo,
      &JsonSidecarRequest {
        image_item_id: ids[0],
        image_path: PathBuf::from("/data/photo.jpg"),
        output_dir: Some(out_dir.clone()),
      },
    )
    .unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("备注"));
    assert!(content.contains("position"));
    let _ = std::fs::remove_dir_all(out_dir);
  }
}
