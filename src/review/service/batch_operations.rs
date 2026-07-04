//! 批量评审操作：状态、标注、备注（纯 service 层，可单测）。

use crate::review::domain::image_item::ReviewStatus;
use crate::review::error::ReviewResult;
use crate::review::storage::traits::{AnnotationTemplate, RemarkWriteMode, ReviewStorage};

/// 批量修改评审状态请求。
#[derive(Debug, Clone)]
pub struct BatchStatusRequest {
  pub image_ids: Vec<i64>,
  pub target_status: ReviewStatus,
  /// 为 `true` 时忽略不可逆状态变更警告并执行。
  pub confirm_irreversible: bool,
}

/// 单条批量失败记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchItemFailure {
  pub image_id: i64,
  pub reason: String,
}

/// 不可逆状态变更警告（需二次确认）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTransitionWarning {
  pub image_id: i64,
  pub from: ReviewStatus,
  pub to: ReviewStatus,
  pub message: String,
}

/// 批量修改状态结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchStatusResult {
  pub success_count: usize,
  pub failures: Vec<BatchItemFailure>,
  pub warnings: Vec<StatusTransitionWarning>,
  /// 是否已写入数据库（存在警告且未确认时为 false）。
  pub applied: bool,
}

/// 批量添加标注请求。
#[derive(Debug, Clone)]
pub struct BatchAnnotateRequest {
  pub image_ids: Vec<i64>,
  pub template: AnnotationTemplate,
}

/// 批量添加标注结果（`annotation_ids` 可用于撤销）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchAnnotateResult {
  pub success_count: usize,
  pub annotation_ids: Vec<i64>,
  pub failures: Vec<BatchItemFailure>,
}

/// 批量添加备注请求。
#[derive(Debug, Clone)]
pub struct BatchRemarkRequest {
  pub image_ids: Vec<i64>,
  pub text: String,
  pub mode: RemarkWriteMode,
}

/// 批量备注结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRemarkResult {
  pub success_count: usize,
  pub failures: Vec<BatchItemFailure>,
}

/// 批量评审操作编排。
pub struct BatchOperations<'a, S: ReviewStorage + ?Sized> {
  storage: &'a S,
}

impl<'a, S: ReviewStorage + ?Sized> BatchOperations<'a, S> {
  pub fn new(storage: &'a S) -> Self {
    Self { storage }
  }

  /// 批量修改评审状态（事务：全部成功才提交）。
  pub fn batch_update_status(&self, request: &BatchStatusRequest) -> ReviewResult<BatchStatusResult> {
    if request.image_ids.is_empty() {
      return Ok(BatchStatusResult {
        success_count: 0,
        failures: Vec::new(),
        warnings: Vec::new(),
        applied: false,
      });
    }

    let items = self.storage.get_images_by_ids(&request.image_ids)?;
    let mut failures = Vec::new();
    let found: std::collections::HashSet<i64> = items.iter().map(|i| i.id).collect();
    for id in &request.image_ids {
      if !found.contains(id) {
        failures.push(BatchItemFailure {
          image_id: *id,
          reason: "图片项不存在".into(),
        });
      }
    }
    if !failures.is_empty() {
      return Ok(BatchStatusResult {
        success_count: 0,
        failures,
        warnings: Vec::new(),
        applied: false,
      });
    }

    let warnings = collect_status_warnings(&items, request.target_status);
    if !warnings.is_empty() && !request.confirm_irreversible {
      return Ok(BatchStatusResult {
        success_count: 0,
        failures: Vec::new(),
        warnings,
        applied: false,
      });
    }

    self
      .storage
      .batch_set_status(&request.image_ids, request.target_status)?;

    Ok(BatchStatusResult {
      success_count: request.image_ids.len(),
      failures: Vec::new(),
      warnings,
      applied: true,
    })
  }

  /// 批量添加相同标注（事务原子性）。
  pub fn batch_add_annotations(
    &self,
    request: &BatchAnnotateRequest,
  ) -> ReviewResult<BatchAnnotateResult> {
    if request.image_ids.is_empty() {
      return Ok(BatchAnnotateResult {
        success_count: 0,
        annotation_ids: Vec::new(),
        failures: Vec::new(),
      });
    }

    let items = self.storage.get_images_by_ids(&request.image_ids)?;
    let found: std::collections::HashSet<i64> = items.iter().map(|i| i.id).collect();
    let mut failures = Vec::new();
    for id in &request.image_ids {
      if !found.contains(id) {
        failures.push(BatchItemFailure {
          image_id: *id,
          reason: "图片项不存在".into(),
        });
      }
    }
    if !failures.is_empty() {
      return Ok(BatchAnnotateResult {
        success_count: 0,
        annotation_ids: Vec::new(),
        failures,
      });
    }

    let annotation_ids = self.storage.batch_insert_annotation_template(
      &request.template,
      &request.image_ids,
    )?;

    Ok(BatchAnnotateResult {
      success_count: request.image_ids.len(),
      annotation_ids,
      failures: Vec::new(),
    })
  }

  /// 撤销批量添加的标注（按 `batch_add_annotations` 返回的 id 列表）。
  pub fn undo_batch_annotations(&self, annotation_ids: &[i64]) -> ReviewResult<()> {
    if annotation_ids.is_empty() {
      return Ok(());
    }
    self.storage.delete_annotations_by_ids(annotation_ids)
  }

  /// 批量添加备注（追加或覆盖）。
  pub fn batch_add_remarks(&self, request: &BatchRemarkRequest) -> ReviewResult<BatchRemarkResult> {
    if request.image_ids.is_empty() {
      return Ok(BatchRemarkResult {
        success_count: 0,
        failures: Vec::new(),
      });
    }

    let items = self.storage.get_images_by_ids(&request.image_ids)?;
    let found: std::collections::HashSet<i64> = items.iter().map(|i| i.id).collect();
    let mut failures = Vec::new();
    for id in &request.image_ids {
      if !found.contains(id) {
        failures.push(BatchItemFailure {
          image_id: *id,
          reason: "图片项不存在".into(),
        });
      }
    }
    if !failures.is_empty() {
      return Ok(BatchRemarkResult {
        success_count: 0,
        failures,
      });
    }

    self
      .storage
      .batch_set_remarks(&request.image_ids, &request.text, request.mode)?;

    Ok(BatchRemarkResult {
      success_count: request.image_ids.len(),
      failures: Vec::new(),
    })
  }
}

/// 检测不可逆状态变更（如 驳回 → 通过）。
pub fn is_irreversible_transition(from: ReviewStatus, to: ReviewStatus) -> bool {
  matches!((from, to), (ReviewStatus::Rejected, ReviewStatus::Approved))
}

fn collect_status_warnings(
  items: &[crate::review::domain::image_item::ReviewImageItem],
  target: ReviewStatus,
) -> Vec<StatusTransitionWarning> {
  items
    .iter()
    .filter(|item| is_irreversible_transition(item.status, target))
    .map(|item| StatusTransitionWarning {
      image_id: item.id,
      from: item.status,
      to: target,
      message: format!(
        "图片 {} 将从「{}」变更为「{}」，该操作不可逆",
        item.file_path.display(),
        item.status.label(),
        target.label()
      ),
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use crate::review::domain::annotation::{
    AnnotationKind, AnnotationPosition, AnnotationStyle, RectanglePosition,
  };
  use crate::review::domain::image_item::ReviewStatus;
  use crate::review::storage::SqliteReviewRepository;
  use crate::review::storage::traits::{AnnotationTemplate, RemarkWriteMode};

  use super::*;

  fn fixture_repo() -> (SqliteReviewRepository, i64) {
    let repo = SqliteReviewRepository::open_memory().expect("memory db");
    let paths = vec![
      PathBuf::from("/tmp/review_test/a.jpg"),
      PathBuf::from("/tmp/review_test/b.jpg"),
    ];
    let batch_id = repo.create_batch("test", &paths).expect("batch");
    (repo, batch_id)
  }

  fn image_ids(repo: &SqliteReviewRepository, batch_id: i64) -> Vec<i64> {
    repo.list_images(batch_id, &Default::default())
      .unwrap()
      .into_iter()
      .map(|i| i.id)
      .collect()
  }

  #[test]
  fn batch_status_rejected_to_approved_requires_confirm() {
    let (repo, batch_id) = fixture_repo();
    let ids = image_ids(&repo, batch_id);
    repo.batch_set_status(&[ids[0]], ReviewStatus::Rejected)
      .unwrap();

    let ops = BatchOperations::new(&repo);
    let result = ops
      .batch_update_status(&BatchStatusRequest {
        image_ids: vec![ids[0]],
        target_status: ReviewStatus::Approved,
        confirm_irreversible: false,
      })
      .unwrap();

    assert!(!result.applied);
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(
      repo.get_image(ids[0]).unwrap().status,
      ReviewStatus::Rejected
    );
  }

  #[test]
  fn batch_status_applies_after_confirm() {
    let (repo, batch_id) = fixture_repo();
    let ids = image_ids(&repo, batch_id);
    repo.batch_set_status(&[ids[0]], ReviewStatus::Rejected)
      .unwrap();

    let ops = BatchOperations::new(&repo);
    let result = ops
      .batch_update_status(&BatchStatusRequest {
        image_ids: vec![ids[0]],
        target_status: ReviewStatus::Approved,
        confirm_irreversible: true,
      })
      .unwrap();

    assert!(result.applied);
    assert_eq!(result.success_count, 1);
    assert_eq!(
      repo.get_image(ids[0]).unwrap().status,
      ReviewStatus::Approved
    );
  }

  #[test]
  fn batch_remark_append_mode() {
    let (repo, batch_id) = fixture_repo();
    let ids = image_ids(&repo, batch_id);
    repo.batch_set_remarks(&[ids[0]], "第一行", RemarkWriteMode::Overwrite)
      .unwrap();

    let ops = BatchOperations::new(&repo);
    ops
      .batch_add_remarks(&BatchRemarkRequest {
        image_ids: vec![ids[0]],
        text: "第二行".into(),
        mode: RemarkWriteMode::Append,
      })
      .unwrap();

    let remark = repo.get_image(ids[0]).unwrap().remark;
    assert!(remark.contains("第一行"));
    assert!(remark.contains("第二行"));
  }

  #[test]
  fn batch_annotate_and_undo() {
    let (repo, batch_id) = fixture_repo();
    let ids = image_ids(&repo, batch_id);
    let ops = BatchOperations::new(&repo);

    let template = AnnotationTemplate {
      kind: AnnotationKind::Rectangle,
      position: AnnotationPosition::Rectangle(RectanglePosition {
        x0: 0.1,
        y0: 0.1,
        x1: 0.5,
        y1: 0.5,
      }),
      style: AnnotationStyle::default(),
      content: "偏暗".into(),
    };

    let result = ops
      .batch_add_annotations(&BatchAnnotateRequest {
        image_ids: ids.clone(),
        template,
      })
      .unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.annotation_ids.len(), 2);
    assert_eq!(repo.list_annotations(ids[0]).unwrap().len(), 1);

    ops.undo_batch_annotations(&result.annotation_ids).unwrap();
    assert!(repo.list_annotations(ids[0]).unwrap().is_empty());
  }

  #[test]
  fn is_irreversible_only_rejected_to_approved() {
    assert!(is_irreversible_transition(
      ReviewStatus::Rejected,
      ReviewStatus::Approved
    ));
    assert!(!is_irreversible_transition(
      ReviewStatus::Pending,
      ReviewStatus::Approved
    ));
  }
}
