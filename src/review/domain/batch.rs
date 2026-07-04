//! 评审批次实体。

use chrono::{DateTime, Utc};

use super::image_item::ReviewStatus;

/// 评审批次。
#[derive(Debug, Clone)]
pub struct ReviewBatch {
  pub id: i64,
  pub name: String,
  pub total_count: i32,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

/// 批次内各状态数量统计。
#[derive(Debug, Clone, Default)]
pub struct BatchStats {
  pub pending: i32,
  pub approved: i32,
  pub needs_fix: i32,
  pub rejected: i32,
}

impl BatchStats {
  pub fn total(&self) -> i32 {
    self.pending + self.approved + self.needs_fix + self.rejected
  }

  pub fn increment(&mut self, status: ReviewStatus) {
    match status {
      ReviewStatus::Pending => self.pending += 1,
      ReviewStatus::Approved => self.approved += 1,
      ReviewStatus::NeedsFix => self.needs_fix += 1,
      ReviewStatus::Rejected => self.rejected += 1,
    }
  }
}
