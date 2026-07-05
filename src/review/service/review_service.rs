//! 评审核心业务编排。

use std::path::{Path, PathBuf};

use crate::review::domain::annotation::{Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle};
use crate::review::domain::convert_params::ConvertParams;
use crate::review::domain::image_item::{ImageFilter, ReviewImageItem, ReviewStatus};
use crate::review::domain::metadata::{read_image_metadata, ImageMetadata};
use crate::review::error::{ReviewError, ReviewResult};
use crate::review::service::batch_operations::{
  BatchAnnotateRequest, BatchAnnotateResult, BatchOperations, BatchRemarkRequest,
  BatchRemarkResult, BatchStatusRequest, BatchStatusResult,
};
use crate::review::service::batch_service::BatchService;
use crate::review::service::conversion_bridge::ReviewConversionBridge;
use crate::review::service::export_service::{CsvExportRequest, ExportService, JsonSidecarRequest};
use crate::review::service::shortcuts::ShortcutConfig;
use crate::review::service::thumbnail_service::ThumbnailService;
use crate::review::storage::sqlite_repository::{ensure_cache_dirs, SqliteReviewRepository};
use crate::review::storage::traits::{AnnotationTemplate, RemarkWriteMode};

/// 评审主服务：编排批次、标注、状态、导出与转换联动。
pub struct ReviewService {
  repo: SqliteReviewRepository,
  pub shortcuts: ShortcutConfig,
}

impl ReviewService {
  pub fn open() -> ReviewResult<Self> {
    ensure_cache_dirs()?;
    let repo = SqliteReviewRepository::open()?;
    if let Err(e) = crate::review::storage::consistency::repair(repo.connection()) {
      tracing::warn!("review consistency repair: {e}");
    }
    if let Err(e) = crate::review::storage::backup::create_backup() {
      tracing::debug!("review startup backup skipped: {e}");
    }
    let shortcuts = ShortcutConfig::load().unwrap_or_default();
    Ok(Self { repo, shortcuts })
  }

  /// 使用主项目已有 SQLite 连接打开评审服务。
  pub fn with_connection(conn: rusqlite::Connection) -> ReviewResult<Self> {
    ensure_cache_dirs()?;
    let repo = SqliteReviewRepository::new(conn)?;
    if let Err(e) = crate::review::storage::consistency::repair(repo.connection()) {
      tracing::warn!("review consistency repair: {e}");
    }
    let shortcuts = ShortcutConfig::load().unwrap_or_default();
    Ok(Self { repo, shortcuts })
  }

  pub fn repo(&self) -> &SqliteReviewRepository {
    &self.repo
  }

  pub fn batch_service(&self) -> BatchService<'_> {
    BatchService::new(self.repo())
  }

  pub fn restore_session(&self) -> ReviewResult<(Option<i64>, Option<i64>)> {
    self.repo.load_session()
  }

  pub fn save_session(&self, batch_id: i64, image_id: i64) -> ReviewResult<()> {
    self.repo.save_session(batch_id, image_id)
  }

  pub fn list_images(&self, batch_id: i64, filter: &ImageFilter) -> ReviewResult<Vec<ReviewImageItem>> {
    self.repo.list_images(batch_id, filter)
  }

  pub fn load_annotations(&self, image_id: i64) -> ReviewResult<Vec<Annotation>> {
    self.repo.list_annotations(image_id)
  }

  pub fn set_status(&self, image_id: i64, status: ReviewStatus) -> ReviewResult<()> {
    self.repo.update_image_status(image_id, status)
  }

  pub fn set_remark(&self, image_id: i64, remark: &str) -> ReviewResult<()> {
    self.repo.update_image_remark(image_id, remark)
  }

  pub fn add_annotation(&self, ann: &Annotation) -> ReviewResult<i64> {
    self.repo.insert_annotation(ann)
  }

  pub fn undo_last_annotation(&self, image_id: i64) -> ReviewResult<()> {
    self.repo.delete_last_annotation(image_id)
  }

  pub fn remove_annotation(&self, id: i64) -> ReviewResult<()> {
    self.repo.delete_annotation(id)
  }

  pub fn update_annotation_position(
    &self,
    id: i64,
    position: &AnnotationPosition,
  ) -> ReviewResult<()> {
    self.repo.update_annotation_position(id, position)
  }

  pub fn update_annotation_content(&self, id: i64, content: &str) -> ReviewResult<()> {
    self.repo.update_annotation_content(id, content)
  }

  pub fn batch_ops(&self) -> BatchOperations<'_, SqliteReviewRepository> {
    BatchOperations::new(&self.repo)
  }

  pub fn batch_update_status(&self, request: &BatchStatusRequest) -> ReviewResult<BatchStatusResult> {
    self.batch_ops().batch_update_status(request)
  }

  pub fn batch_add_annotations(
    &self,
    request: &BatchAnnotateRequest,
  ) -> ReviewResult<BatchAnnotateResult> {
    self.batch_ops().batch_add_annotations(request)
  }

  pub fn undo_batch_annotations(&self, annotation_ids: &[i64]) -> ReviewResult<()> {
    self.batch_ops().undo_batch_annotations(annotation_ids)
  }

  pub fn batch_add_remarks(&self, request: &BatchRemarkRequest) -> ReviewResult<BatchRemarkResult> {
    self.batch_ops().batch_add_remarks(request)
  }

  /// 兼容旧接口：批量设状态（无不可逆确认）。
  pub fn batch_set_status(&self, ids: &[i64], status: ReviewStatus) -> ReviewResult<()> {
    let result = self.batch_update_status(&BatchStatusRequest {
      image_ids: ids.to_vec(),
      target_status: status,
      confirm_irreversible: true,
    })?;
    if !result.failures.is_empty() {
      return Err(ReviewError::Message(result.failures[0].reason.clone()));
    }
    Ok(())
  }

  pub fn batch_clear_annotations(&self, ids: &[i64]) -> ReviewResult<()> {
    self.repo.batch_clear_annotations(ids)
  }

  /// 兼容旧接口：批量覆盖备注。
  pub fn batch_add_remark(&self, ids: &[i64], remark: &str) -> ReviewResult<()> {
    let result = self.batch_add_remarks(&BatchRemarkRequest {
      image_ids: ids.to_vec(),
      text: remark.to_string(),
      mode: RemarkWriteMode::Overwrite,
    })?;
    if !result.failures.is_empty() {
      return Err(ReviewError::Message(result.failures[0].reason.clone()));
    }
    Ok(())
  }

  pub fn batch_add_annotation(
    &self,
    template: &Annotation,
    target_ids: &[i64],
  ) -> ReviewResult<()> {
    let tpl = AnnotationTemplate {
      kind: template.kind,
      position: template.position.clone(),
      style: template.style.clone(),
      content: template.content.clone(),
    };
    let result = self.batch_add_annotations(&BatchAnnotateRequest {
      image_ids: target_ids.to_vec(),
      template: tpl,
    })?;
    if !result.failures.is_empty() {
      return Err(ReviewError::Message(result.failures[0].reason.clone()));
    }
    Ok(())
  }

  pub fn ensure_thumbnail(&self, image_id: i64, source: &Path) -> ReviewResult<PathBuf> {
    ThumbnailService::ensure_thumbnail(&self.repo, image_id, source)
  }

  pub fn export_csv(&self, batch_id: i64, dest: &Path) -> ReviewResult<()> {
    ExportService::export_batch_csv_legacy(&self.repo, batch_id, dest)
  }

  pub fn export_csv_with_request(
    &self,
    request: &CsvExportRequest,
  ) -> ReviewResult<crate::review::service::export_service::CsvExportResult> {
    ExportService::export_csv(&self.repo, request)
  }

  pub fn export_sidecar(&self, image_id: i64, path: &Path) -> ReviewResult<PathBuf> {
    ExportService::export_annotation_json(
      &self.repo,
      &JsonSidecarRequest {
        image_item_id: image_id,
        image_path: path.to_path_buf(),
        output_dir: None,
      },
    )
  }

  pub fn create_batch_from_folder(
    &self,
    name: &str,
    folder: &Path,
    recursive: bool,
  ) -> ReviewResult<i64> {
    self.batch_service().create_from_folder(name, folder, recursive)
  }

  pub fn create_batch_from_paths(&self, name: &str, paths: &[PathBuf]) -> ReviewResult<i64> {
    self.batch_service().create_from_paths(name, paths)
  }
}

impl ReviewConversionBridge for ReviewService {
  fn approved_paths(&self, batch_id: i64) -> ReviewResult<Vec<PathBuf>> {
    self.repo.approved_paths_in_batch(batch_id)
  }

  fn approved_with_params(
    &self,
    batch_id: i64,
  ) -> ReviewResult<Vec<crate::review::service::conversion_bridge::ConversionTaskParams>> {
    Ok(
      self
        .repo
        .approved_items_with_params_in_batch(batch_id)?
        .into_iter()
        .map(
          |(path, params)| crate::review::service::conversion_bridge::ConversionTaskParams {
            path,
            params,
          },
        )
        .collect(),
    )
  }

  fn status_for_path(&self, path: &Path) -> ReviewResult<Option<ReviewStatus>> {
    self.repo.status_for_path(path)
  }

  fn burn_annotations_for_export(
    &self,
    source: &Path,
    output: &Path,
    quality: u8,
  ) -> ReviewResult<()> {
    use image::DynamicImage;
    use crate::core::types::{ImageFormat, Quality};
    use crate::processing::backends::native_backend::encode_dynamic_image;

    let path_str = source.to_string_lossy();
    let image_id = self
      .repo
      .connection()
      .query_row(
        "SELECT id FROM review_image_item WHERE file_path = ?1",
        [path_str.as_ref()],
        |row| row.get::<_, i64>(0),
      )
      .map_err(|_| ReviewError::NotFound {
        entity: "review_image_item",
        id: 0,
      })?;
    let annotations = self.repo.list_annotations(image_id)?;
    let img = image::open(output)
      .map_err(|e| ReviewError::ImageDecode {
        path: output.to_path_buf(),
        source: e,
      })?;
    let mut rgba = img.to_rgba8();
    crate::review::domain::burn_annotations_onto(&mut rgba, &annotations);

    let target_format = output
      .extension()
      .and_then(|ext| ext.to_str())
      .and_then(ImageFormat::from_extension)
      .unwrap_or(ImageFormat::Jpeg);
    let encoded = encode_dynamic_image(
      &DynamicImage::ImageRgba8(rgba),
      target_format,
      Quality::new(quality).unwrap_or(Quality::DEFAULT),
    )
    .map_err(|e| ReviewError::Message(e.to_string()))?;
    std::fs::write(output, encoded)?;
    Ok(())
  }

  fn export_annotation_sidecar(&self, image_item_id: i64, image_path: &Path) -> ReviewResult<PathBuf> {
    ExportService::export_annotation_json(
      &self.repo,
      &JsonSidecarRequest {
        image_item_id,
        image_path: image_path.to_path_buf(),
        output_dir: None,
      },
    )
  }
}

impl ReviewService {
  pub fn update_convert_params(&self, id: i64, params: &ConvertParams) -> ReviewResult<()> {
    self.repo.update_convert_params(id, params)
  }

  // ── 自定义标签 ────────────────────────────────────────

  pub fn list_tags(&self) -> ReviewResult<Vec<crate::review::domain::ReviewTag>> {
    self.repo.list_tags()
  }

  pub fn create_tag(&self, name: &str, color: [u8; 4]) -> ReviewResult<i64> {
    self.repo.create_tag(name, color)
  }

  pub fn rename_tag(&self, id: i64, name: &str) -> ReviewResult<()> {
    self.repo.rename_tag(id, name)
  }

  pub fn delete_tag(&self, id: i64) -> ReviewResult<()> {
    self.repo.delete_tag(id)
  }

  pub fn tags_for_image(&self, image_id: i64) -> ReviewResult<Vec<i64>> {
    self.repo.tags_for_image(image_id)
  }

  pub fn set_image_tag(&self, image_id: i64, tag_id: i64, on: bool) -> ReviewResult<()> {
    self.repo.set_image_tag(image_id, tag_id, on)
  }

  pub fn tags_for_images(
    &self,
    image_ids: &[i64],
  ) -> ReviewResult<std::collections::HashMap<i64, Vec<i64>>> {
    self.repo.tags_for_images(image_ids)
  }

  pub fn load_metadata(&self, path: &Path) -> ReviewResult<ImageMetadata> {
    read_image_metadata(path)
  }

  pub fn refresh_metadata_cache(&self, id: i64, path: &Path) -> ReviewResult<ImageMetadata> {
    let meta = read_image_metadata(path)?;
    self.repo.update_image_metadata(id, meta.file_size, meta.width, meta.height)?;
    Ok(meta)
  }

  pub fn soft_delete_image(&self, id: i64) -> ReviewResult<()> {
    self.repo.soft_delete_image(id)
  }

  pub fn restore_image(&self, id: i64) -> ReviewResult<()> {
    self.repo.restore_image(id)
  }

  pub fn soft_delete_batch(&self, id: i64) -> ReviewResult<()> {
    self.repo.soft_delete_batch(id)
  }

  pub fn list_deleted_images(&self, batch_id: i64) -> ReviewResult<Vec<ReviewImageItem>> {
    self.repo.list_deleted_images(batch_id)
  }

  pub fn save_shortcuts(&mut self, config: &ShortcutConfig) -> ReviewResult<()> {
    config.save()?;
    self.shortcuts = config.clone();
    Ok(())
  }
}

/// 批量标注模板构建辅助。
pub fn make_template(
  image_item_id: i64,
  kind: AnnotationKind,
  position: AnnotationPosition,
  style: AnnotationStyle,
  content: String,
) -> Annotation {
  Annotation::new_draft(image_item_id, kind, position, style, content)
}
