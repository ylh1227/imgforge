//! 全模块远程数据模型：资产、批次、标注、提取结果、任务规格。

use serde::{Deserialize, Serialize};

use crate::remote::types::{RemoteAssetRef, RemoteJobSource, REMOTE_SCHEMA_VERSION};

/// 远程资产（对象存储中的文件）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteAsset {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl From<&RemoteAsset> for RemoteAssetRef {
    fn from(a: &RemoteAsset) -> Self {
        Self {
            id: a.id.clone(),
            name: a.name.clone(),
            mime: a.mime.clone(),
            size: a.size,
            checksum: a.checksum.clone(),
            download_url: a.download_url.clone(),
        }
    }
}

/// 任务产物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteArtifact {
    pub schema_version: u32,
    pub id: String,
    pub job_id: String,
    pub kind: String,
    pub asset: RemoteAssetRef,
    pub created_at: u64,
}

/// 评审批次种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteBatchKind {
    #[default]
    Image,
    Video,
}

/// 远程评审批次。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteBatch {
    pub schema_version: u32,
    pub batch_id: String,
    pub name: String,
    pub kind: RemoteBatchKind,
    pub source: RemoteJobSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub item_count: usize,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_asset: Option<RemoteAssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_summary: Option<String>,
}

/// 评审条目状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteReviewItemStatus {
    #[default]
    Pending,
    Approved,
    NeedsFix,
    Rejected,
}

impl RemoteReviewItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::NeedsFix => "needs_fix",
            Self::Rejected => "rejected",
        }
    }
}

/// 图片/视频评审条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteReviewItem {
    pub schema_version: u32,
    pub item_id: String,
    pub batch_id: String,
    pub asset: RemoteAssetRef,
    pub status: RemoteReviewItemStatus,
    #[serde(default)]
    pub remark: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_asset: Option<RemoteAssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_asset: Option<RemoteAssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    pub updated_at: u64,
}

/// 标注类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAnnotationKind {
    Rectangle,
    Arrow,
    Text,
    Marker,
    Segment,
}

/// 远程标注（图片框选 / 视频时间点）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteAnnotation {
    pub schema_version: u32,
    pub annotation_id: String,
    pub item_id: String,
    pub kind: RemoteAnnotationKind,
    #[serde(default)]
    pub content: String,
    /// 归一化几何或时间：图片用 x0,y0,x1,y1；视频用 time_ms / start_ms/end_ms。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geometry: Vec<(String, f64)>,
    pub created_at: u64,
    #[serde(default)]
    pub locked: bool,
}

/// 数据提取数据集 / 结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteExtractDataset {
    pub schema_version: u32,
    pub result_id: String,
    pub batch_name: String,
    pub module: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub record_count: usize,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_asset: Option<RemoteAssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_csv_asset: Option<RemoteAssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// 报表引用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteReport {
    pub schema_version: u32,
    pub report_id: String,
    pub kind: String,
    pub title: String,
    pub asset: RemoteAssetRef,
    pub created_at: u64,
}

/// 转换任务规格。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ConvertJobSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recursive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve_structure: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bayer_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_max_bytes: Option<u64>,
}

/// 图片评审任务规格。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ReviewJobSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default)]
    pub generate_thumbnails: bool,
    #[serde(default)]
    pub generate_previews: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag_set: Vec<String>,
}

/// 视频评审任务规格。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VideoReviewJobSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default)]
    pub probe: bool,
    #[serde(default)]
    pub extract_cover: bool,
    #[serde(default)]
    pub contact_sheet: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_interval_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_sheet_cols: Option<u32>,
}

/// 数据提取任务规格。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DataExtractJobSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_name: Option<String>,
    #[serde(default)]
    pub enable_ocr: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_formats: Vec<String>,
}

/// 按 source 分发的任务规格信封。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteJobSpec {
    Convert(ConvertJobSpec),
    Review(ReviewJobSpec),
    VideoReview(VideoReviewJobSpec),
    DataExtract(DataExtractJobSpec),
}

impl RemoteJobSpec {
    pub fn source(&self) -> RemoteJobSource {
        match self {
            Self::Convert(_) => RemoteJobSource::Convert,
            Self::Review(_) => RemoteJobSource::Review,
            Self::VideoReview(_) => RemoteJobSource::VideoReview,
            Self::DataExtract(_) => RemoteJobSource::DataExtract,
        }
    }
}

/// 创建评审批次请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateRemoteBatchRequest {
    pub schema_version: u32,
    pub name: String,
    pub kind: RemoteBatchKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub asset_ids: Vec<String>,
}

impl Default for CreateRemoteBatchRequest {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            name: String::new(),
            kind: RemoteBatchKind::Image,
            workspace_id: None,
            asset_ids: Vec::new(),
        }
    }
}

/// 更新评审条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UpdateRemoteReviewItemRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<RemoteReviewItemStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// API 路径常量。
pub const VIDEO_BATCHES_PATH: &str = "/v1/video/batches";
pub const REVIEW_ITEMS_PATH_TEMPLATE: &str = "/v1/review/batches/{id}/items";
pub const REVIEW_ANNOTATIONS_PATH_TEMPLATE: &str = "/v1/review/items/{id}/annotations";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_spec_roundtrip() {
        let spec = RemoteJobSpec::Review(ReviewJobSpec {
            batch_name: Some("b1".into()),
            generate_thumbnails: true,
            ..Default::default()
        });
        let json = serde_json::to_string(&spec).unwrap();
        let back: RemoteJobSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source(), RemoteJobSource::Review);
    }
}
