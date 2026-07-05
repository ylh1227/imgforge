//! 评审模块用户配置（默认不改变原有习惯）。

use serde::{Deserialize, Serialize};

use crate::review::domain::image_item::ReviewStatus;
use crate::review::error::ReviewResult;
use crate::review::storage::paths::review_config_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewModuleConfig {
  /// 切换状态后自动跳下一张未评审图片。
  #[serde(default)]
  pub auto_advance_on_status: bool,
  /// 自动跳转时查找的目标状态（默认未评审）。
  #[serde(default = "default_advance_target")]
  pub auto_advance_target: ReviewStatus,
  /// 前后邻居预加载数量。
  #[serde(default = "default_prefetch")]
  pub prefetch_neighbors: usize,
  /// 纹理缓存最大条目数（LRU）。
  #[serde(default = "default_texture_entries")]
  pub texture_cache_max_entries: usize,
  /// 定时备份间隔（分钟，0=禁用）。
  #[serde(default = "default_backup_interval")]
  pub backup_interval_minutes: u32,
  /// 对比视图默认模式（single/split/wipe/overlay/diff）。
  #[serde(default)]
  pub default_compare_mode: String,
  /// 是否启用差异高亮。
  #[serde(default)]
  pub diff_highlight: bool,
}

fn default_advance_target() -> ReviewStatus {
  ReviewStatus::Pending
}

fn default_prefetch() -> usize {
  2
}

fn default_texture_entries() -> usize {
  24
}

fn default_backup_interval() -> u32 {
  30
}

impl Default for ReviewModuleConfig {
  fn default() -> Self {
    Self {
      auto_advance_on_status: false,
      auto_advance_target: ReviewStatus::Pending,
      prefetch_neighbors: 2,
      texture_cache_max_entries: 24,
      backup_interval_minutes: 30,
      default_compare_mode: "single".into(),
      diff_highlight: false,
    }
  }
}

impl ReviewModuleConfig {
  pub fn load() -> ReviewResult<Self> {
    let path = review_config_path()?;
    if !path.exists() {
      return Ok(Self::default());
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
  }

  pub fn save(&self) -> ReviewResult<()> {
    let path = review_config_path()?;
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
    Ok(())
  }
}
