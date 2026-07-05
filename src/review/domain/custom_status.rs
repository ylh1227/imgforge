//! 自定义评审状态标签（可绑定转换参数）。

use serde::{Deserialize, Serialize};

use crate::review::domain::convert_params::ConvertParams;

/// 用户自定义状态标签（存储于 SQLite，向后兼容内置四态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomStatusLabel {
  pub id: i64,
  pub name: String,
  pub color: [u8; 4],
  pub maps_to: Option<String>,
  pub convert_params: ConvertParams,
  pub sort_order: i32,
}

impl CustomStatusLabel {
  pub fn display_name(&self) -> &str {
    &self.name
  }
}
