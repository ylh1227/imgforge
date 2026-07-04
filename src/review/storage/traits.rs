//! 评审存储抽象：service 层通过 trait 访问数据库，便于测试与替换实现。

use crate::review::domain::annotation::{Annotation, AnnotationKind, AnnotationPosition, AnnotationStyle};
use crate::review::domain::image_item::{ImageFilter, ReviewImageItem, ReviewStatus};
use crate::review::error::ReviewResult;

/// 备注写入模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemarkWriteMode {
  Append,
  Overwrite,
}

/// 标注模板（批量打标输入）。
#[derive(Debug, Clone)]
pub struct AnnotationTemplate {
  pub kind: AnnotationKind,
  pub position: AnnotationPosition,
  pub style: AnnotationStyle,
  pub content: String,
}

/// CSV 导出行查询条件。
#[derive(Debug, Clone)]
pub struct ExportRowsQuery {
  pub batch_id: i64,
  pub status_filter: Option<ReviewStatus>,
  pub image_ids: Option<Vec<i64>>,
}

/// CSV 导出行数据。
#[derive(Debug, Clone)]
pub struct ReviewExportRow {
  pub file_name: String,
  pub file_path: String,
  pub status: ReviewStatus,
  pub remark: String,
  pub annotation_count: i32,
  pub updated_at: String,
}

/// 评审数据存储接口（由 `SqliteReviewRepository` 实现，扩展 service 层批量/导出能力）。
pub trait ReviewStorage {
  fn get_image(&self, id: i64) -> ReviewResult<ReviewImageItem>;
  fn get_images_by_ids(&self, ids: &[i64]) -> ReviewResult<Vec<ReviewImageItem>>;
  fn list_images(&self, batch_id: i64, filter: &ImageFilter) -> ReviewResult<Vec<ReviewImageItem>>;

  /// 事务批量更新状态；任一 id 不存在则整批回滚。
  fn batch_set_status(&self, ids: &[i64], status: ReviewStatus) -> ReviewResult<()>;

  /// 事务批量更新备注（追加或覆盖）。
  fn batch_set_remarks(
    &self,
    ids: &[i64],
    text: &str,
    mode: RemarkWriteMode,
  ) -> ReviewResult<()>;

  /// 事务批量插入相同标注模板，返回新标注 id 列表（供撤销）。
  fn batch_insert_annotation_template(
    &self,
    template: &AnnotationTemplate,
    image_ids: &[i64],
  ) -> ReviewResult<Vec<i64>>;

  /// 按 id 批量删除标注（撤销批量打标）。
  fn delete_annotations_by_ids(&self, annotation_ids: &[i64]) -> ReviewResult<()>;

  fn list_annotations(&self, image_item_id: i64) -> ReviewResult<Vec<Annotation>>;

  fn list_export_rows(&self, query: &ExportRowsQuery) -> ReviewResult<Vec<ReviewExportRow>>;
}
