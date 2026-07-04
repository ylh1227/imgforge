//! 评审图片条目与状态枚举。

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
}

/// 列表筛选条件。
#[derive(Debug, Clone, Default)]
pub struct ImageFilter {
  pub status: Option<ReviewStatus>,
  pub search: String,
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
}
