//! ffprobe / ffmpeg 外部命令后端。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use serde::Deserialize;

use crate::video_review::domain::VideoMetadata;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};

#[derive(Debug, Clone)]
pub struct FfmpegConfig {
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
}

impl Default for FfmpegConfig {
    fn default() -> Self {
        Self {
            ffmpeg_path: "ffmpeg".into(),
            ffprobe_path: "ffprobe".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FfmpegAvailability {
    pub ffmpeg_ok: bool,
    pub ffprobe_ok: bool,
    pub ffmpeg_version: Option<String>,
    pub ffprobe_version: Option<String>,
}

pub trait VideoBackend: Send + Sync {
    fn availability(&self) -> FfmpegAvailability;
    fn probe_metadata(&self, path: &Path) -> VideoReviewResult<VideoMetadata>;
    fn extract_frame(
        &self,
        path: &Path,
        time_ms: u64,
        width: u32,
        output: &Path,
    ) -> VideoReviewResult<()>;
}

pub struct FfmpegBackend {
    config: FfmpegConfig,
    availability: Mutex<Option<FfmpegAvailability>>,
}

impl FfmpegBackend {
    pub fn new(config: FfmpegConfig) -> Self {
        Self {
            config,
            availability: Mutex::new(None),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(FfmpegConfig::default())
    }

    fn check_tool(path: &str) -> (bool, Option<String>) {
        let output = Command::new(path).arg("-version").output();
        match output {
            Ok(out) if out.status.success() => {
                let first = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                (true, if first.is_empty() { None } else { Some(first) })
            }
            _ => (false, None),
        }
    }

    fn cached_availability(&self) -> FfmpegAvailability {
        let mut guard = self.availability.lock().unwrap();
        if let Some(ref a) = *guard {
            return a.clone();
        }
        let (ffmpeg_ok, ffmpeg_version) = Self::check_tool(&self.config.ffmpeg_path);
        let (ffprobe_ok, ffprobe_version) = Self::check_tool(&self.config.ffprobe_path);
        let avail = FfmpegAvailability {
            ffmpeg_ok,
            ffprobe_ok,
            ffmpeg_version,
            ffprobe_version,
        };
        *guard = Some(avail.clone());
        avail
    }
}

impl VideoBackend for FfmpegBackend {
    fn availability(&self) -> FfmpegAvailability {
        self.cached_availability()
    }

    fn probe_metadata(&self, path: &Path) -> VideoReviewResult<VideoMetadata> {
        let avail = self.cached_availability();
        if !avail.ffprobe_ok {
            return Err(VideoReviewError::FfmpegUnavailable(
                "ffprobe 未安装或不在 PATH 中".into(),
            ));
        }

        let output = Command::new(&self.config.ffprobe_path)
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
                path.to_string_lossy().as_ref(),
            ])
            .output()
            .map_err(|e| VideoReviewError::FfprobeFailed {
                path: path.to_path_buf(),
                detail: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(VideoReviewError::FfprobeFailed {
                path: path.to_path_buf(),
                detail: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        parse_ffprobe_json(&output.stdout).map_err(|e| VideoReviewError::FfprobeFailed {
            path: path.to_path_buf(),
            detail: e,
        })
    }

    fn extract_frame(
        &self,
        path: &Path,
        time_ms: u64,
        width: u32,
        output: &Path,
    ) -> VideoReviewResult<()> {
        let avail = self.cached_availability();
        if !avail.ffmpeg_ok {
            return Err(VideoReviewError::FfmpegUnavailable(
                "ffmpeg 未安装或不在 PATH 中".into(),
            ));
        }

        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let seconds = time_ms as f64 / 1000.0;
        let scale = if width > 0 {
            format!("scale={width}:-2")
        } else {
            String::new()
        };

        let mut cmd = Command::new(&self.config.ffmpeg_path);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{seconds:.3}"),
        ]);
        cmd.args(["-i", path.to_string_lossy().as_ref()]);
        cmd.args(["-frames:v", "1"]);
        if !scale.is_empty() {
            cmd.args(["-vf", &scale]);
        }
        cmd.args(["-y", output.to_string_lossy().as_ref()]);

        let output_proc = cmd
            .output()
            .map_err(|e| VideoReviewError::FrameExtractFailed {
                path: path.to_path_buf(),
                detail: e.to_string(),
            })?;

        if !output_proc.status.success() {
            return Err(VideoReviewError::FrameExtractFailed {
                path: path.to_path_buf(),
                detail: String::from_utf8_lossy(&output_proc.stderr).to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeRoot {
    streams: Option<Vec<FfprobeStream>>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default)]
    avg_frame_rate: String,
    #[serde(default)]
    r_frame_rate: String,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
    bit_rate: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

pub fn parse_ffprobe_json(bytes: &[u8]) -> Result<VideoMetadata, String> {
    let root: FfprobeRoot =
        serde_json::from_slice(bytes).map_err(|e| format!("JSON 解析失败: {e}"))?;

    let mut meta = VideoMetadata::default();
    if let Some(fmt) = root.format {
        meta.device_model = device_model_from_tags(&fmt.tags);
        if let Some(dur) = fmt.duration.and_then(|d| d.parse::<f64>().ok()) {
            meta.duration_ms = (dur * 1000.0).round() as u64;
        }
        if let Some(br) = fmt.bit_rate.and_then(|b| b.parse::<u64>().ok()) {
            meta.bitrate_kbps = Some((br / 1000) as u32);
        }
    }

    if let Some(streams) = root.streams {
        for stream in streams {
            let kind = stream.codec_type.as_deref().unwrap_or("");
            if kind == "video" && meta.width == 0 {
                meta.width = stream.width.unwrap_or(0);
                meta.height = stream.height.unwrap_or(0);
                meta.video_codec = stream.codec_name.unwrap_or_default();
                meta.fps = parse_frame_rate(&stream.avg_frame_rate)
                    .or_else(|| parse_frame_rate(&stream.r_frame_rate))
                    .unwrap_or(0.0);
            } else if kind == "audio" && meta.audio_codec.is_none() {
                meta.audio_codec = stream.codec_name;
            }
            if meta.device_model.is_none() {
                meta.device_model = device_model_from_tags(&stream.tags);
            }
        }
    }

    Ok(meta)
}

pub fn device_model_from_tags(tags: &BTreeMap<String, String>) -> Option<String> {
    let value = |key: &str| {
        tags.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    };

    let make = value("make");
    let model = value("model")
        .or_else(|| value("com.apple.quicktime.model"))
        .or_else(|| value("com.android.model"));
    if let Some(model) = model {
        if let Some(make) = make {
            if !model
                .to_ascii_lowercase()
                .contains(&make.to_ascii_lowercase())
            {
                return Some(format!("{make} {model}"));
            }
        }
        return Some(model.to_string());
    }

    value("encoder")
        .or_else(|| value("com.apple.quicktime.software"))
        .map(str::to_string)
}

pub fn parse_frame_rate(raw: &str) -> Option<f32> {
    if raw.is_empty() || raw == "0/0" {
        return None;
    }
    if let Some((num, den)) = raw.split_once('/') {
        let n: f32 = num.parse().ok()?;
        let d: f32 = den.parse().ok()?;
        if d > 0.0 {
            return Some(n / d);
        }
        return None;
    }
    raw.parse().ok()
}

pub fn ms_to_timestamp(ms: u64) -> String {
    let total = ms / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    let frac = ms % 1000;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}.{frac:03}")
    } else {
        format!("{m:02}:{s:02}.{frac:03}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frame_rate_fraction() {
        assert!((parse_frame_rate("30000/1001").unwrap() - 29.97).abs() < 0.01);
        assert_eq!(parse_frame_rate("24/1"), Some(24.0));
    }

    #[test]
    fn parse_ffprobe_minimal() {
        let json = r#"{
      "streams": [
        {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"24/1"},
        {"codec_type":"audio","codec_name":"aac"}
      ],
      "format": {"duration":"120.5","bit_rate":"8000000"}
    }"#;
        let meta = parse_ffprobe_json(json.as_bytes()).unwrap();
        assert_eq!(meta.duration_ms, 120_500);
        assert_eq!(meta.width, 1920);
        assert!((meta.fps - 24.0).abs() < 0.01);
        assert_eq!(meta.audio_codec.as_deref(), Some("aac"));
    }

    #[test]
    fn parse_ffprobe_device_model_from_tags() {
        let json = r#"{
      "streams": [
        {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"24/1"}
      ],
      "format": {
        "duration":"1",
        "tags": {"com.apple.quicktime.model":"iPhone 15 Pro"}
      }
    }"#;
        let meta = parse_ffprobe_json(json.as_bytes()).unwrap();
        assert_eq!(meta.device_model.as_deref(), Some("iPhone 15 Pro"));
    }
}
