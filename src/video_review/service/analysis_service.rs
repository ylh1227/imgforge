//! 视频评审规则型分析：生成可确认的标记与标签建议。

use serde::{Deserialize, Serialize};

use crate::video_review::domain::VideoItem;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoAnalysisSuggestion {
    pub video_id: i64,
    pub time_ms: u64,
    pub text: String,
    pub severity: u8,
    pub tag_hint: Option<String>,
}

pub struct VideoAnalysisService;

impl VideoAnalysisService {
    pub fn suggest_for_video(video: &VideoItem) -> Vec<VideoAnalysisSuggestion> {
        let mut suggestions = Vec::new();
        if video.fps > 0.0 && video.fps < 20.0 {
            suggestions.push(suggestion(
                video,
                0,
                format!("低帧率：{:.2} fps", video.fps),
                2,
                Some("低帧率"),
            ));
        }
        if video.width == 0 || video.height == 0 {
            suggestions.push(suggestion(
                video,
                0,
                "分辨率元数据缺失",
                2,
                Some("元数据异常"),
            ));
        } else if video.width < 640 || video.height < 360 {
            suggestions.push(suggestion(
                video,
                0,
                format!("低分辨率：{}×{}", video.width, video.height),
                1,
                Some("低分辨率"),
            ));
        }
        if video.duration_ms > 10 * 60 * 1000 {
            suggestions.push(suggestion(
                video,
                video.duration_ms / 2,
                "素材时长超过 10 分钟，建议抽查中段和结尾",
                1,
                Some("长素材"),
            ));
        }
        if video.audio_codec.is_none() {
            suggestions.push(suggestion(video, 0, "未检测到音频流", 1, Some("无音频")));
        }
        if video
            .remark
            .as_deref()
            .is_some_and(|r| r.contains("黑场") || r.to_ascii_lowercase().contains("black"))
        {
            suggestions.push(suggestion(video, 0, "备注疑似黑场问题", 2, Some("黑场")));
        }
        suggestions
    }

    pub fn suggest_tags_from_text(text: &str) -> Vec<String> {
        let lower = text.to_ascii_lowercase();
        let mut tags = Vec::new();
        for (needle, tag) in [
            ("黑场", "黑场"),
            ("black", "黑场"),
            ("字幕", "字幕问题"),
            ("subtitle", "字幕问题"),
            ("音画", "音画同步"),
            ("sync", "音画同步"),
            ("抖动", "画面抖动"),
            ("shake", "画面抖动"),
        ] {
            if lower.contains(&needle.to_ascii_lowercase()) && !tags.iter().any(|t| t == tag) {
                tags.push(tag.to_string());
            }
        }
        tags
    }
}

fn suggestion(
    video: &VideoItem,
    time_ms: u64,
    text: impl Into<String>,
    severity: u8,
    tag_hint: Option<&str>,
) -> VideoAnalysisSuggestion {
    VideoAnalysisSuggestion {
        video_id: video.id,
        time_ms,
        text: text.into(),
        severity,
        tag_hint: tag_hint.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::domain::image_item::ReviewStatus;
    use chrono::Utc;
    use std::path::PathBuf;

    fn video() -> VideoItem {
        VideoItem {
            id: 1,
            batch_id: 1,
            file_path: PathBuf::from("/tmp/a.mp4"),
            status: ReviewStatus::Pending,
            remark: None,
            thumbnail_path: None,
            duration_ms: 30_000,
            fps: 12.0,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            audio_codec: None,
            bitrate_kbps: None,
            device_model: None,
            offset_ms: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn suggests_low_fps_and_missing_audio() {
        let suggestions = VideoAnalysisService::suggest_for_video(&video());
        assert!(suggestions.iter().any(|s| s.text.contains("低帧率")));
        assert!(suggestions.iter().any(|s| s.text.contains("音频")));
    }
}
