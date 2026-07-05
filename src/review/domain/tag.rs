//! 自定义问题标签（与基础评审状态独立，多对多绑定图片）。

use chrono::{DateTime, Utc};

/// 用户自定义问题标签。
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewTag {
  pub id: i64,
  pub name: String,
  pub color: [u8; 4],
  pub created_at: DateTime<Utc>,
}

impl ReviewTag {
  /// 默认标签色板（新建标签时轮换取色）。
  pub fn palette() -> [[u8; 4]; 8] {
    [
      [255, 59, 48, 255],
      [255, 149, 0, 255],
      [255, 204, 0, 255],
      [52, 199, 89, 255],
      [0, 199, 190, 255],
      [0, 122, 255, 255],
      [175, 82, 222, 255],
      [142, 142, 147, 255],
    ]
  }
}

/// 标签筛选模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TagFilterMode {
  /// 包含任意所选标签。
  #[default]
  Any,
  /// 包含全部所选标签。
  All,
}

impl TagFilterMode {
  pub fn label(self) -> &'static str {
    match self {
      Self::Any => "包含任意",
      Self::All => "包含全部",
    }
  }
}
