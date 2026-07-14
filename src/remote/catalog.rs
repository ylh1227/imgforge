//! 服务器数据加载：素材、评审批次、数据提取结果列表契约。

use serde::{Deserialize, Serialize};

use crate::remote::types::{RemoteAssetRef, RemoteJobSource, REMOTE_SCHEMA_VERSION};

/// 通用分页查询。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePageQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

fn default_limit() -> usize {
    50
}

impl Default for RemotePageQuery {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            offset: 0,
            workspace_id: None,
            cursor: None,
        }
    }
}

/// 分页响应。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemotePage<T> {
    pub schema_version: u32,
    pub items: Vec<T>,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl<T> RemotePage<T> {
    pub fn new(items: Vec<T>, total: usize) -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            items,
            total,
            next_cursor: None,
        }
    }
}

/// 素材列表项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteAssetListItem {
    pub asset: RemoteAssetRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// 评审批次摘要（图片/视频共用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteReviewBatchSummary {
    pub batch_id: String,
    pub name: String,
    pub source: RemoteJobSource,
    pub item_count: usize,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_asset: Option<RemoteAssetRef>,
}

/// 数据提取结果摘要。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteExtractResultSummary {
    pub result_id: String,
    pub module: String,
    pub batch_name: String,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_asset: Option<RemoteAssetRef>,
}

/// 数据加载路径常量。
pub const ASSETS_PATH: &str = "/v1/assets";
pub const REVIEW_BATCHES_PATH: &str = "/v1/review/batches";
pub const EXTRACT_RESULTS_PATH: &str = "/v1/extract/results";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_defaults() {
        let q = RemotePageQuery::default();
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
    }
}
