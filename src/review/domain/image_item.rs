//! 评审图片条目与状态枚举。

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::review::domain::convert_params::ConvertParams;

/// 单图评审状态（类型安全，禁止字符串表示业务状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(i32)]
pub enum ReviewStatus {
  Pending = 0,
  Approved = 1,
  NeedsFix = 2,
  Rejected = 3,
}

impl ReviewStatus {
  pub fn from_db(value: i32) -> Option<Self> {
    match value {
      0 => Some(Self::Pending),
      1 => Some(Self::Approved),
      2 => Some(Self::NeedsFix),
      3 => Some(Self::Rejected),
      _ => None,
    }
  }

  pub fn db_value(self) -> i32 {
    self as i32
  }

  /// 持久化层 TEXT 列值（pending/passed/need_fix/rejected）。
  pub fn to_sql(self) -> &'static str {
    match self {
      Self::Pending => "pending",
      Self::Approved => "passed",
      Self::NeedsFix => "need_fix",
      Self::Rejected => "rejected",
    }
  }

  pub fn from_sql(value: &str) -> Option<Self> {
    match value {
      "pending" => Some(Self::Pending),
      "passed" => Some(Self::Approved),
      "need_fix" => Some(Self::NeedsFix),
      "rejected" => Some(Self::Rejected),
      _ => None,
    }
  }

  pub fn label(self) -> &'static str {
    match self {
      Self::Pending => "未评审",
      Self::Approved => "通过",
      Self::NeedsFix => "待修正",
      Self::Rejected => "驳回",
    }
  }

  pub fn shortcut_digit(self) -> Option<u8> {
    match self {
      Self::Pending => Some(0),
      Self::Approved => Some(1),
      Self::NeedsFix => Some(2),
      Self::Rejected => Some(3),
    }
  }

  pub fn from_digit(digit: u8) -> Option<Self> {
    match digit {
      0 => Some(Self::Pending),
      1 => Some(Self::Approved),
      2 => Some(Self::NeedsFix),
      3 => Some(Self::Rejected),
      _ => None,
    }
  }

  pub fn all() -> [Self; 4] {
    [
      Self::Pending,
      Self::Approved,
      Self::NeedsFix,
      Self::Rejected,
    ]
  }
}

/// 列表排序键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImageSortKey {
  #[default]
  FilePath,
  Status,
  UpdatedAt,
  FileSize,
  Resolution,
  AnnotationCount,
}

impl ImageSortKey {
  pub fn label(self) -> &'static str {
    match self {
      Self::FilePath => "文件名",
      Self::Status => "状态",
      Self::UpdatedAt => "更新时间",
      Self::FileSize => "文件大小",
      Self::Resolution => "分辨率",
      Self::AnnotationCount => "标注数",
    }
  }
}

/// 列表筛选与排序条件。
#[derive(Debug, Clone, Default)]
pub struct ImageFilter {
  pub status: Option<ReviewStatus>,
  pub search: String,
  pub sort_by: ImageSortKey,
  pub sort_asc: bool,
  pub min_annotations: Option<i32>,
  pub remark_contains: String,
  pub include_deleted: bool,
}

impl ImageFilter {
  pub fn apply_in_memory(&self, items: &mut Vec<ReviewImageItem>) {
    let search = self.search.trim().to_ascii_lowercase();
    let remark = self.remark_contains.trim().to_ascii_lowercase();
    items.retain(|i| {
      if !self.include_deleted && i.is_deleted() {
        return false;
      }
      if let Some(s) = self.status {
        if i.status != s {
          return false;
        }
      }
      if !search.is_empty()
        && !i
          .file_path
          .to_string_lossy()
          .to_ascii_lowercase()
          .contains(&search)
      {
        return false;
      }
      if !remark.is_empty() && !i.remark.to_ascii_lowercase().contains(&remark) {
        return false;
      }
      if let Some(min) = self.min_annotations {
        if i.annotation_count < min {
          return false;
        }
      }
      true
    });
    items.sort_by(|a, b| {
      let ord = match self.sort_by {
        ImageSortKey::FilePath => a
          .file_path
          .to_string_lossy()
          .cmp(&b.file_path.to_string_lossy()),
        ImageSortKey::Status => a.status.db_value().cmp(&b.status.db_value()),
        ImageSortKey::UpdatedAt => a.updated_at.cmp(&b.updated_at),
        ImageSortKey::FileSize => a.file_size.unwrap_or(0).cmp(&b.file_size.unwrap_or(0)),
        ImageSortKey::Resolution => {
          let ar = a.width.unwrap_or(0) as u64 * a.height.unwrap_or(0) as u64;
          let br = b.width.unwrap_or(0) as u64 * b.height.unwrap_or(0) as u64;
          ar.cmp(&br)
        }
        ImageSortKey::AnnotationCount => a.annotation_count.cmp(&b.annotation_count),
      };
      if self.sort_asc {
        ord
      } else {
        ord.reverse()
      }
    });
  }
}

/// 评审图片条目。
#[derive(Debug, Clone)]
pub struct ReviewImageItem {
  pub id: i64,
  pub batch_id: i64,
  pub file_path: PathBuf,
  pub status: ReviewStatus,
  pub remark: String,
  pub thumbnail_path: Option<PathBuf>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub deleted_at: Option<DateTime<Utc>>,
  pub file_size: Option<u64>,
  pub width: Option<u32>,
  pub height: Option<u32>,
  pub convert_params: ConvertParams,
  pub annotation_count: i32,
}

impl ReviewImageItem {
  pub fn is_deleted(&self) -> bool {
    self.deleted_at.is_some()
  }
}

/// 查找下一张未评审（或指定状态）图片 id。
pub fn next_image_id(
  images: &[ReviewImageItem],
  current_id: i64,
  target: ReviewStatus,
) -> Option<i64> {
  let pos = images.iter().position(|i| i.id == current_id)?;
  images
    .iter()
    .skip(pos + 1)
    .chain(images.iter().take(pos))
    .find(|i| !i.is_deleted() && i.status == target)
    .map(|i| i.id)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn next_image_wraps() {
    let items = vec![
      make_item(1, ReviewStatus::Pending),
      make_item(2, ReviewStatus::Approved),
      make_item(3, ReviewStatus::Pending),
    ];
    assert_eq!(next_image_id(&items, 1, ReviewStatus::Pending), Some(3));
  }

  fn make_item(id: i64, status: ReviewStatus) -> ReviewImageItem {
    ReviewImageItem {
      id,
      batch_id: 1,
      file_path: PathBuf::from(format!("/tmp/{id}.jpg")),
      status,
      remark: String::new(),
      thumbnail_path: None,
      created_at: Utc::now(),
      updated_at: Utc::now(),
      deleted_at: None,
      file_size: None,
      width: None,
      height: None,
      convert_params: ConvertParams::default(),
      annotation_count: 0,
    }
  }
}
