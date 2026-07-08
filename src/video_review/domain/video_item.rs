//! 视频条目。

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::review::domain::image_item::ReviewStatus;

use super::metadata::VideoMetadata;

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v"];

#[derive(Debug, Clone)]
pub struct VideoItem {
    pub id: i64,
    pub batch_id: i64,
    pub file_path: PathBuf,
    pub status: ReviewStatus,
    pub remark: Option<String>,
    pub thumbnail_path: Option<PathBuf>,
    pub duration_ms: u64,
    pub fps: f32,
    pub width: u32,
    pub height: u32,
    pub video_codec: String,
    pub audio_codec: Option<String>,
    pub bitrate_kbps: Option<u32>,
    pub device_model: Option<String>,
    pub offset_ms: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl VideoItem {
    pub fn metadata(&self) -> VideoMetadata {
        VideoMetadata {
            duration_ms: self.duration_ms,
            fps: self.fps,
            width: self.width,
            height: self.height,
            video_codec: self.video_codec.clone(),
            audio_codec: self.audio_codec.clone(),
            bitrate_kbps: self.bitrate_kbps,
            device_model: self.device_model.clone(),
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub fn effective_time_ms(&self, global_ms: u64) -> u64 {
        let shifted = global_ms as i64 + self.offset_ms;
        shifted.max(0) as u64
    }
}

#[derive(Debug, Clone, Default)]
pub struct VideoFilter {
    pub status: Option<ReviewStatus>,
    pub search: String,
    pub tag_ids: Vec<i64>,
    pub include_deleted: bool,
}

impl VideoFilter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn apply_in_memory(&self, items: &mut Vec<VideoItem>) {
        if let Some(status) = self.status {
            items.retain(|i| i.status == status);
        }
        if !self.search.is_empty() {
            let query = VideoQuery::parse(&self.search);
            items.retain(|i| query.matches(i));
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoQuery {
    pub status: Option<ReviewStatus>,
    pub text: Option<String>,
    pub min_duration_ms: Option<u64>,
    pub max_duration_ms: Option<u64>,
}

impl VideoQuery {
    pub fn parse(input: &str) -> Self {
        let mut query = Self::default();
        let lower = input.to_ascii_lowercase();
        if lower.contains("待修正") || lower.contains("needs_fix") {
            query.status = Some(ReviewStatus::NeedsFix);
        } else if lower.contains("驳回") || lower.contains("rejected") {
            query.status = Some(ReviewStatus::Rejected);
        } else if lower.contains("通过") || lower.contains("approved") || lower.contains("passed")
        {
            query.status = Some(ReviewStatus::Approved);
        } else if lower.contains("未评审") || lower.contains("pending") {
            query.status = Some(ReviewStatus::Pending);
        }
        for token in input.split_whitespace() {
            if let Some(value) = token.strip_prefix("status:") {
                query.status = status_from_text(value);
            } else if let Some(value) = token.strip_prefix("duration>") {
                query.min_duration_ms = parse_duration_ms(value);
            } else if let Some(value) = token.strip_prefix("duration<") {
                query.max_duration_ms = parse_duration_ms(value);
            } else if let Some(value) = token.strip_prefix("text:") {
                query.text = Some(value.to_ascii_lowercase());
            }
        }
        if query.text.is_none()
            && query.status.is_none()
            && query.min_duration_ms.is_none()
            && query.max_duration_ms.is_none()
        {
            query.text = Some(lower);
        }
        query
    }

    pub fn matches(&self, item: &VideoItem) -> bool {
        if let Some(status) = self.status {
            if item.status != status {
                return false;
            }
        }
        if let Some(min) = self.min_duration_ms {
            if item.duration_ms <= min {
                return false;
            }
        }
        if let Some(max) = self.max_duration_ms {
            if item.duration_ms >= max {
                return false;
            }
        }
        if let Some(text) = &self.text {
            let haystack = format!(
                "{} {} {} {} {}x{}",
                item.file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(""),
                item.remark.as_deref().unwrap_or(""),
                item.device_model.as_deref().unwrap_or(""),
                item.video_codec,
                item.width,
                item.height
            )
            .to_ascii_lowercase();
            if !haystack.contains(text) {
                return false;
            }
        }
        true
    }
}

fn status_from_text(input: &str) -> Option<ReviewStatus> {
    match input {
        "待修正" | "needs_fix" | "need_fix" => Some(ReviewStatus::NeedsFix),
        "驳回" | "rejected" => Some(ReviewStatus::Rejected),
        "通过" | "approved" | "passed" => Some(ReviewStatus::Approved),
        "未评审" | "pending" => Some(ReviewStatus::Pending),
        _ => None,
    }
}

fn parse_duration_ms(input: &str) -> Option<u64> {
    let raw = input.trim();
    if let Some(sec) = raw.strip_suffix('s') {
        sec.parse::<f64>().ok().map(|s| (s * 1000.0) as u64)
    } else if let Some(min) = raw.strip_suffix('m') {
        min.parse::<f64>().ok().map(|m| (m * 60_000.0) as u64)
    } else {
        raw.parse::<u64>().ok()
    }
}

pub fn is_video_extension(ext: &str) -> bool {
    VIDEO_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn item() -> VideoItem {
        VideoItem {
            id: 1,
            batch_id: 1,
            file_path: PathBuf::from("/tmp/black_scene.mp4"),
            status: ReviewStatus::NeedsFix,
            remark: Some("黑场".into()),
            thumbnail_path: None,
            duration_ms: 20_000,
            fps: 24.0,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            audio_codec: Some("aac".into()),
            bitrate_kbps: None,
            device_model: Some("iPhone 15 Pro".into()),
            offset_ms: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn structured_video_query_matches() {
        let q = VideoQuery::parse("status:待修正 duration>10s");
        assert!(q.matches(&item()));
        let q = VideoQuery::parse("duration<10s");
        assert!(!q.matches(&item()));
    }
}
