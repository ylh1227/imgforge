//! 将远程 catalog 映射为本地评审 UI 可用的结构（合成 i64 id + 缓存路径）。

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{TimeZone, Utc};

use crate::remote::catalog::RemoteReviewBatchSummary;
use crate::remote::models::{RemoteReviewItem, RemoteReviewItemStatus};
use crate::remote::types::RemoteAssetRef;
use crate::review::domain::convert_params::ConvertParams;
use crate::review::domain::{BatchStats, ReviewBatch, ReviewImageItem, ReviewStatus};

/// 远程字符串 id ↔ 合成负整数 id。
#[derive(Debug, Default, Clone)]
pub struct RemoteIdMap {
    to_local: HashMap<String, i64>,
    to_remote: HashMap<i64, String>,
    next: i64,
}

impl RemoteIdMap {
    pub fn new() -> Self {
        Self {
            to_local: HashMap::new(),
            to_remote: HashMap::new(),
            next: -1,
        }
    }

    pub fn clear(&mut self) {
        self.to_local.clear();
        self.to_remote.clear();
        self.next = -1;
    }

    pub fn intern(&mut self, remote_id: &str) -> i64 {
        if let Some(id) = self.to_local.get(remote_id) {
            return *id;
        }
        let id = self.next;
        self.next -= 1;
        self.to_local.insert(remote_id.to_string(), id);
        self.to_remote.insert(id, remote_id.to_string());
        id
    }

    pub fn remote_of(&self, local: i64) -> Option<&str> {
        self.to_remote.get(&local).map(|s| s.as_str())
    }

    pub fn local_of(&self, remote_id: &str) -> Option<i64> {
        self.to_local.get(remote_id).copied()
    }
}

pub fn remote_status_to_local(s: RemoteReviewItemStatus) -> ReviewStatus {
    match s {
        RemoteReviewItemStatus::Pending => ReviewStatus::Pending,
        RemoteReviewItemStatus::Approved => ReviewStatus::Approved,
        RemoteReviewItemStatus::NeedsFix => ReviewStatus::NeedsFix,
        RemoteReviewItemStatus::Rejected => ReviewStatus::Rejected,
    }
}

pub fn local_status_to_remote(s: ReviewStatus) -> RemoteReviewItemStatus {
    match s {
        ReviewStatus::Pending => RemoteReviewItemStatus::Pending,
        ReviewStatus::Approved => RemoteReviewItemStatus::Approved,
        ReviewStatus::NeedsFix => RemoteReviewItemStatus::NeedsFix,
        ReviewStatus::Rejected => RemoteReviewItemStatus::Rejected,
    }
}

pub fn batch_from_summary(
    map: &mut RemoteIdMap,
    summary: &RemoteReviewBatchSummary,
) -> ReviewBatch {
    let id = map.intern(&summary.batch_id);
    let ts = Utc
        .timestamp_opt(summary.updated_at as i64, 0)
        .single()
        .unwrap_or_else(Utc::now);
    ReviewBatch {
        id,
        name: summary.name.clone(),
        total_count: summary.item_count as i32,
        created_at: ts,
        updated_at: ts,
    }
}

pub fn stats_from_items(items: &[RemoteReviewItem]) -> BatchStats {
    let mut stats = BatchStats::default();
    for item in items {
        stats.increment(remote_status_to_local(item.status));
    }
    stats
}

pub fn image_from_remote_item(
    map: &mut RemoteIdMap,
    batch_local_id: i64,
    item: &RemoteReviewItem,
    file_path: PathBuf,
    thumb_path: Option<PathBuf>,
) -> ReviewImageItem {
    let id = map.intern(&item.item_id);
    let ts = Utc
        .timestamp_opt(item.updated_at as i64, 0)
        .single()
        .unwrap_or_else(Utc::now);
    ReviewImageItem {
        id,
        batch_id: batch_local_id,
        file_path,
        status: remote_status_to_local(item.status),
        remark: item.remark.clone(),
        thumbnail_path: thumb_path,
        created_at: ts,
        updated_at: ts,
        deleted_at: None,
        file_size: item.asset.size,
        width: item.width,
        height: item.height,
        convert_params: ConvertParams::default(),
        annotation_count: 0,
    }
}

pub fn placeholder_path_for_asset(asset: &RemoteAssetRef) -> PathBuf {
    PathBuf::from(format!("remote://{}/{}", asset.id, asset.name))
}
