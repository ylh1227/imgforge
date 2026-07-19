//! 评审持久化 trait 与仓储 DTO（与领域模型解耦的查询/写入类型）。

use std::path::PathBuf;

use crate::review::domain::annotation::Annotation;
use crate::review::domain::batch::ReviewBatch;
use crate::review::domain::image_item::{ImageFilter, ReviewImageItem, ReviewStatus};
use crate::review::error::ReviewError;

/// 新建图片项（写入仓储层）。
#[derive(Debug, Clone)]
pub struct NewReviewImageItem {
    pub file_path: PathBuf,
    pub thumbnail_path: Option<PathBuf>,
}

/// 图片项列表筛选。
#[derive(Debug, Clone, Default)]
pub struct ItemFilter {
    pub status: Option<ReviewStatus>,
    pub search: String,
}

impl From<&ImageFilter> for ItemFilter {
    fn from(value: &ImageFilter) -> Self {
        Self {
            status: value.status,
            search: value.search.clone(),
        }
    }
}

/// 评审数据仓储接口（由 SQLite 实现，与领域模型通过映射解耦）。
pub trait ReviewRepository {
    // ── 批次 ──────────────────────────────────────────────

    fn create_batch(&self, name: &str) -> Result<i64, ReviewError>;
    fn list_batches(&self) -> Result<Vec<ReviewBatch>, ReviewError>;
    fn get_batch(&self, id: i64) -> Result<ReviewBatch, ReviewError>;
    fn delete_batch(&self, id: i64) -> Result<(), ReviewError>;

    // ── 图片项 ────────────────────────────────────────────

    fn add_image_items(
        &self,
        batch_id: i64,
        items: &[NewReviewImageItem],
    ) -> Result<(), ReviewError>;
    fn list_image_items(
        &self,
        batch_id: i64,
        filter: ItemFilter,
    ) -> Result<Vec<ReviewImageItem>, ReviewError>;
    fn update_image_status(&self, item_id: i64, status: ReviewStatus) -> Result<(), ReviewError>;
    fn update_image_remark(&self, item_id: i64, remark: &str) -> Result<(), ReviewError>;
    fn update_image_jira(
        &self,
        item_id: i64,
        issue_key: &str,
        browse_url: Option<&str>,
    ) -> Result<(), ReviewError>;

    // ── 标注 ──────────────────────────────────────────────

    fn list_annotations(&self, image_item_id: i64) -> Result<Vec<Annotation>, ReviewError>;
    fn add_annotation(
        &self,
        image_item_id: i64,
        annotation: &Annotation,
    ) -> Result<i64, ReviewError>;
    fn delete_annotation(&self, annotation_id: i64) -> Result<(), ReviewError>;
    fn clear_annotations(&self, image_item_id: i64) -> Result<(), ReviewError>;

    // ── 批量（事务）────────────────────────────────────────

    fn batch_update_status(
        &self,
        item_ids: &[i64],
        status: ReviewStatus,
    ) -> Result<usize, ReviewError>;
    fn batch_add_annotation(
        &self,
        item_ids: &[i64],
        annotation: &Annotation,
    ) -> Result<usize, ReviewError>;
}
