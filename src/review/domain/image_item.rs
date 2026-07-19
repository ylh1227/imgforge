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

    /// 全局统一状态色值（RGBA）：灰/绿/橙/红。
    pub fn color_rgba(self) -> [u8; 4] {
        match self {
            Self::Pending => [142, 142, 147, 255],
            Self::Approved => [52, 199, 89, 255],
            Self::NeedsFix => [255, 149, 0, 255],
            Self::Rejected => [255, 59, 48, 255],
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

/// 标注数量筛选维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnnotationFilter {
    #[default]
    Any,
    None,
    Has,
    AtLeast,
}

impl AnnotationFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "不限",
            Self::None => "无标注",
            Self::Has => "有标注",
            Self::AtLeast => "≥ N 个",
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
    /// 标注数量筛选模式。
    pub annotation_filter: AnnotationFilter,
    /// 分辨率宽度下限/上限（像素）。
    pub min_width: Option<u32>,
    pub max_width: Option<u32>,
    /// 分辨率高度下限/上限（像素）。
    pub min_height: Option<u32>,
    pub max_height: Option<u32>,
    /// 文件大小下限/上限（字节）。
    pub min_file_size: Option<u64>,
    pub max_file_size: Option<u64>,
    /// 需包含的标签 id（按 tag_mode 组合）。
    pub tag_ids: Vec<i64>,
    pub tag_mode: crate::review::domain::tag::TagFilterMode,
}

impl ImageFilter {
    /// 重置为默认筛选（保留排序键）。
    pub fn reset_filters(&mut self) {
        let sort_by = self.sort_by;
        let sort_asc = self.sort_asc;
        *self = Self::default();
        self.sort_by = sort_by;
        self.sort_asc = sort_asc;
    }

    /// 按标签维度过滤（需图片-标签映射）。
    pub fn retain_by_tags(
        &self,
        items: &mut Vec<ReviewImageItem>,
        image_tags: &std::collections::HashMap<i64, Vec<i64>>,
    ) {
        if self.tag_ids.is_empty() {
            return;
        }
        let wanted = &self.tag_ids;
        let mode = self.tag_mode;
        items.retain(|i| {
            let empty = Vec::new();
            let tags = image_tags.get(&i.id).unwrap_or(&empty);
            match mode {
                crate::review::domain::tag::TagFilterMode::Any => {
                    wanted.iter().any(|t| tags.contains(t))
                }
                crate::review::domain::tag::TagFilterMode::All => {
                    wanted.iter().all(|t| tags.contains(t))
                }
            }
        });
    }

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
            match self.annotation_filter {
                AnnotationFilter::Any => {}
                AnnotationFilter::None => {
                    if i.annotation_count > 0 {
                        return false;
                    }
                }
                AnnotationFilter::Has => {
                    if i.annotation_count == 0 {
                        return false;
                    }
                }
                AnnotationFilter::AtLeast => {
                    if let Some(min) = self.min_annotations {
                        if i.annotation_count < min {
                            return false;
                        }
                    }
                }
            }
            if self.annotation_filter != AnnotationFilter::AtLeast {
                if let Some(min) = self.min_annotations {
                    if i.annotation_count < min {
                        return false;
                    }
                }
            }
            if let Some(minw) = self.min_width {
                if i.width.unwrap_or(0) < minw {
                    return false;
                }
            }
            if let Some(maxw) = self.max_width {
                if i.width.unwrap_or(u32::MAX) > maxw {
                    return false;
                }
            }
            if let Some(minh) = self.min_height {
                if i.height.unwrap_or(0) < minh {
                    return false;
                }
            }
            if let Some(maxh) = self.max_height {
                if i.height.unwrap_or(u32::MAX) > maxh {
                    return false;
                }
            }
            if let Some(mins) = self.min_file_size {
                if i.file_size.unwrap_or(0) < mins {
                    return false;
                }
            }
            if let Some(maxs) = self.max_file_size {
                if i.file_size.unwrap_or(u64::MAX) > maxs {
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
    /// 已关联的 JIRA Issue Key（如 CAM-123）。
    pub jira_issue_key: Option<String>,
    /// JIRA 浏览 URL。
    pub jira_url: Option<String>,
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
            jira_issue_key: None,
            jira_url: None,
        }
    }
}
