//! 与格式转换模块的公共对接接口（低耦合，仅通过路径关联）。

use std::path::{Path, PathBuf};

use crate::review::domain::image_item::ReviewStatus;
use crate::review::domain::ConvertParams;
use crate::review::error::ReviewResult;

/// 转换队列条目（带评审状态标签）。
#[derive(Debug, Clone)]
pub struct ReviewQueueItem {
  pub path: PathBuf,
  pub status: Option<ReviewStatus>,
}

/// 带单图转换参数的入队条目（评审标记联动带入队列）。
#[derive(Debug, Clone)]
pub struct ConversionTaskParams {
  pub path: PathBuf,
  pub params: ConvertParams,
}

/// 格式转换模块对接 trait：评审模块不依赖转换内部实现。
pub trait ReviewConversionBridge {
  /// 获取批次内「通过」状态的图片路径，供加入转换队列。
  fn approved_paths(&self, batch_id: i64) -> ReviewResult<Vec<PathBuf>>;

  /// 获取批次内「通过」图片及其单图转换参数（默认基于 `approved_paths` 拼装，实现方可覆盖）。
  fn approved_with_params(&self, batch_id: i64) -> ReviewResult<Vec<ConversionTaskParams>> {
    Ok(
      self
        .approved_paths(batch_id)?
        .into_iter()
        .map(|path| ConversionTaskParams {
          path,
          params: ConvertParams::default(),
        })
        .collect(),
    )
  }

  /// 查询单文件评审状态（转换列表展示标签）。
  fn status_for_path(&self, path: &Path) -> ReviewResult<Option<ReviewStatus>>;

  /// 导出时可选：将标注烧录到已转换的输出图，并按指定质量重新编码。
  fn burn_annotations_for_export(
    &self,
    source: &Path,
    output: &Path,
    quality: u8,
  ) -> ReviewResult<()>;

  /// 导出标注 JSON 侧载文件（同名 `.json`）。
  fn export_annotation_sidecar(&self, image_item_id: i64, image_path: &Path) -> ReviewResult<PathBuf>;
}
