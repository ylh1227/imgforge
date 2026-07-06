//! 多视频宫格拼接视频导出（ffmpeg xstack）。
//!
//! 默认按各源视频最大分辨率拼格，仅必要时缩小、不放大，并使用高质量编码参数保留清晰度与色彩。

use std::path::PathBuf;
use std::process::Command;

use crate::video_review::domain::VideoItem;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::contact_sheet::grid_dimensions;

/// 预览/UI 占位尺寸；实际导出默认使用源视频分辨率。
pub const DEFAULT_CELL_WIDTH: u32 = 640;
pub const DEFAULT_CELL_HEIGHT: u32 = 360;
pub const DEFAULT_CLIP_DURATION_MS: u64 = 10_000;
pub const MIN_CLIP_DURATION_MS: u64 = 500;

/// 高质量 H.264 编码参数（CRF 越低越接近源画质）。
const EXPORT_CRF: &str = "17";
const EXPORT_PRESET: &str = "slow";
const EXPORT_AUDIO_BITRATE: &str = "192k";
const LOSSLESS_PRESET: &str = "veryslow";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridVideoExportQuality {
  #[default]
  High,
  Lossless,
}

impl GridVideoExportQuality {
  pub fn label(self) -> &'static str {
    match self {
      Self::High => "高质量",
      Self::Lossless => "无损",
    }
  }
}

#[derive(Debug, Clone)]
pub struct GridVideoExportRequest {
  pub videos: Vec<VideoItem>,
  pub start_time_ms: u64,
  pub duration_ms: u64,
  pub dest: PathBuf,
  /// `0` 表示按源视频自动计算，不强制缩小。
  pub cell_width: u32,
  pub cell_height: u32,
  pub quality: GridVideoExportQuality,
}

impl GridVideoExportRequest {
  pub fn new(
    videos: Vec<VideoItem>,
    start_time_ms: u64,
    duration_ms: u64,
    dest: PathBuf,
  ) -> Self {
    Self {
      videos,
      start_time_ms,
      duration_ms,
      dest,
      cell_width: 0,
      cell_height: 0,
      quality: GridVideoExportQuality::default(),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridVideoExportResult {
  pub dest: PathBuf,
  pub width: u32,
  pub height: u32,
  pub duration_ms: u64,
  pub rows: usize,
  pub cols: usize,
  pub video_count: usize,
  pub cell_width: u32,
  pub cell_height: u32,
  pub quality: GridVideoExportQuality,
}

pub struct GridVideoExportService;

impl GridVideoExportService {
  pub fn export(ffmpeg_path: &str, req: &GridVideoExportRequest) -> VideoReviewResult<GridVideoExportResult> {
    if req.videos.len() < 2 {
      return Err(VideoReviewError::Message(
        "视频拼接导出至少需要 2 个视频".into(),
      ));
    }
    if req.videos.len() > 6 {
      return Err(VideoReviewError::Message(
        "视频拼接导出最多支持 6 个视频".into(),
      ));
    }

    let max_dur = max_export_duration_ms(&req.videos, req.start_time_ms);
    if max_dur < MIN_CLIP_DURATION_MS {
      return Err(VideoReviewError::Message(
        "当前时间点之后没有足够时长可导出".into(),
      ));
    }
    let duration_ms = req.duration_ms.min(max_dur);
    if duration_ms < MIN_CLIP_DURATION_MS {
      return Err(VideoReviewError::Message(format!(
        "导出时长过短（至少 {}ms）",
        MIN_CLIP_DURATION_MS
      )));
    }

    let avail = check_ffmpeg(ffmpeg_path)?;
    if !avail {
      return Err(VideoReviewError::FfmpegUnavailable(
        "ffmpeg 未安装或不在 PATH 中".into(),
      ));
    }

    if let Some(parent) = req.dest.parent() {
      std::fs::create_dir_all(parent)?;
    }

    let count = req.videos.len();
    let (rows, cols) = grid_dimensions(count);
    let (cell_w, cell_h) = resolve_cell_size(&req.videos, req.cell_width, req.cell_height);
    let out_w = cols as u32 * cell_w;
    let out_h = rows as u32 * cell_h;

    let filter = build_filter_complex(&req.videos, cols, cell_w, cell_h);
    let duration_sec = duration_ms as f64 / 1000.0;

    let mut cmd = Command::new(ffmpeg_path);
    cmd.args(["-hide_banner", "-loglevel", "error"]);
    for video in &req.videos {
      let ss = video.effective_time_ms(req.start_time_ms) as f64 / 1000.0;
      cmd.args(["-ss", &format!("{ss:.3}")]);
      cmd.args(["-i", video.file_path.to_string_lossy().as_ref()]);
    }
    cmd.args(["-filter_complex", &filter]);
    cmd.args(["-map", "[outv]"]);
    cmd.args(["-map", "0:a?"]);
    cmd.args(["-t", &format!("{duration_sec:.3}")]);
    append_encode_args(&mut cmd, req.quality);
    cmd.args(["-movflags", "+faststart"]);
    cmd.args(["-y", req.dest.to_string_lossy().as_ref()]);

    let output = cmd.output().map_err(|e| VideoReviewError::VideoExportFailed {
      detail: e.to_string(),
    })?;

    if !output.status.success() {
      return Err(VideoReviewError::VideoExportFailed {
        detail: String::from_utf8_lossy(&output.stderr).trim().to_string(),
      });
    }

    if !req.dest.is_file() {
      return Err(VideoReviewError::VideoExportFailed {
        detail: "ffmpeg 完成但未生成输出文件".into(),
      });
    }

    Ok(GridVideoExportResult {
      dest: req.dest.clone(),
      width: out_w,
      height: out_h,
      duration_ms,
      rows,
      cols,
      video_count: count,
      cell_width: cell_w,
      cell_height: cell_h,
      quality: req.quality,
    })
  }
}

fn append_encode_args(cmd: &mut Command, quality: GridVideoExportQuality) {
  cmd.args([
    "-pix_fmt",
    "yuv420p",
    "-colorspace",
    "bt709",
    "-color_primaries",
    "bt709",
    "-color_trc",
    "bt709",
  ]);
  match quality {
    GridVideoExportQuality::High => {
      cmd.args([
        "-c:v",
        "libx264",
        "-preset",
        EXPORT_PRESET,
        "-crf",
        EXPORT_CRF,
        "-c:a",
        "aac",
        "-b:a",
        EXPORT_AUDIO_BITRATE,
        "-ar",
        "48000",
      ]);
    }
    GridVideoExportQuality::Lossless => {
      cmd.args([
        "-c:v",
        "libx264",
        "-preset",
        LOSSLESS_PRESET,
        "-crf",
        "0",
        "-c:a",
        "copy",
      ]);
    }
  }
}

/// 拼格单元尺寸：取各源视频宽高的最大值，保证不放大任何一路画面。
pub fn compute_quality_cell_size(videos: &[VideoItem]) -> (u32, u32) {
  let w = videos
    .iter()
    .map(|v| v.width)
    .filter(|&w| w > 0)
    .max()
    .unwrap_or(DEFAULT_CELL_WIDTH);
  let h = videos
    .iter()
    .map(|v| v.height)
    .filter(|&h| h > 0)
    .max()
    .unwrap_or(DEFAULT_CELL_HEIGHT);
  (w, h)
}

fn resolve_cell_size(videos: &[VideoItem], width: u32, height: u32) -> (u32, u32) {
  if width == 0 || height == 0 {
    compute_quality_cell_size(videos)
  } else {
    (width.max(160), height.max(90))
  }
}

/// 从全局时间点起，各视频剩余可同步导出的最短时长。
pub fn max_export_duration_ms(videos: &[VideoItem], start_ms: u64) -> u64 {
  videos
    .iter()
    .map(|v| {
      let local = v.effective_time_ms(start_ms).min(v.duration_ms);
      v.duration_ms.saturating_sub(local)
    })
    .min()
    .unwrap_or(0)
}

pub fn build_xstack_layout(count: usize, cols: usize, cell_w: u32, cell_h: u32) -> String {
  (0..count)
    .map(|i| {
      let row = i / cols;
      let col = i % cols;
      format!("{}_{}", col as u32 * cell_w, row as u32 * cell_h)
    })
    .collect::<Vec<_>>()
    .join("|")
}

/// 单路视频滤镜：仅在源分辨率大于单元格时缩小（Lanczos），否则仅黑边填充，不放大。
pub fn build_input_video_filter(
  input_idx: usize,
  video: &VideoItem,
  cell_w: u32,
  cell_h: u32,
) -> String {
  let tag = format!("v{input_idx}");
  let common = format!(
    "pad={cell_w}:{cell_h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,setpts=PTS-STARTPTS,format=yuv420p"
  );
  let needs_downscale = video.width > cell_w || video.height > cell_h;
  if needs_downscale {
    format!(
      "[{input_idx}:v]scale={cell_w}:{cell_h}:force_original_aspect_ratio=decrease:flags=lanczos+accurate_rnd+full_chroma_int,{common}[{tag}]"
    )
  } else {
    format!("[{input_idx}:v]{common}[{tag}]")
  }
}

pub fn build_filter_complex(videos: &[VideoItem], cols: usize, cell_w: u32, cell_h: u32) -> String {
  let count = videos.len();
  let mut parts = Vec::with_capacity(count + 1);
  for (i, video) in videos.iter().enumerate() {
    parts.push(build_input_video_filter(i, video, cell_w, cell_h));
  }
  let inputs: String = (0..count).map(|i| format!("[v{i}]")).collect();
  let layout = build_xstack_layout(count, cols, cell_w, cell_h);
  parts.push(format!(
    "{inputs}xstack=inputs={count}:layout={layout}[outv]"
  ));
  parts.join(";")
}

fn check_ffmpeg(path: &str) -> VideoReviewResult<bool> {
  let out = Command::new(path)
    .arg("-version")
    .output()
    .map_err(|e| VideoReviewError::FfmpegUnavailable(e.to_string()))?;
  Ok(out.status.success())
}

#[cfg(test)]
mod tests {
  use super::*;
  use chrono::Utc;
  use crate::review::domain::image_item::ReviewStatus;

  fn sample(id: i64, duration_ms: u64, offset_ms: i64) -> VideoItem {
    sample_with_size(id, duration_ms, offset_ms, 1920, 1080)
  }

  fn sample_with_size(
    id: i64,
    duration_ms: u64,
    offset_ms: i64,
    width: u32,
    height: u32,
  ) -> VideoItem {
    VideoItem {
      id,
      batch_id: 1,
      file_path: PathBuf::from(format!("/tmp/v{id}.mp4")),
      status: ReviewStatus::Pending,
      remark: None,
      thumbnail_path: None,
      duration_ms,
      fps: 24.0,
      width,
      height,
      video_codec: "h264".into(),
      audio_codec: None,
      bitrate_kbps: None,
      offset_ms,
      created_at: Utc::now(),
      updated_at: Utc::now(),
      deleted_at: None,
    }
  }

  #[test]
  fn max_export_duration_respects_offset() {
    let videos = vec![sample(1, 60_000, 0), sample(2, 50_000, -5_000)];
    assert_eq!(max_export_duration_ms(&videos, 10_000), 45_000);
    assert_eq!(max_export_duration_ms(&videos, 50_000), 5_000);
  }

  #[test]
  fn quality_cell_size_uses_max_source_dimensions() {
    let videos = vec![
      sample_with_size(1, 60_000, 0, 1920, 1080),
      sample_with_size(2, 60_000, 0, 1280, 720),
    ];
    assert_eq!(compute_quality_cell_size(&videos), (1920, 1080));
  }

  #[test]
  fn input_filter_avoids_upscale() {
    let video = sample_with_size(1, 60_000, 0, 1280, 720);
    let f = build_input_video_filter(0, &video, 1920, 1080);
    assert!(!f.contains("scale="));
    assert!(f.contains("pad=1920:1080"));
  }

  #[test]
  fn input_filter_downscales_with_lanczos() {
    let video = sample_with_size(1, 60_000, 0, 3840, 2160);
    let f = build_input_video_filter(0, &video, 1920, 1080);
    assert!(f.contains("flags=lanczos+accurate_rnd+full_chroma_int"));
    assert!(f.contains("scale=1920:1080"));
  }

  #[test]
  fn xstack_layout_two_by_two() {
    assert_eq!(build_xstack_layout(4, 2, 640, 360), "0_0|640_0|0_360|640_360");
  }

  #[test]
  fn filter_complex_uses_xstack() {
    let videos = vec![sample(1, 60_000, 0), sample(2, 60_000, 0)];
    let f = build_filter_complex(&videos, 2, 640, 360);
    assert!(f.contains("xstack=inputs=2"));
    assert!(f.contains("[0:v]"));
    assert!(f.contains("[1:v]"));
    assert!(f.contains("[outv]"));
  }

  #[test]
  fn lossless_encode_uses_crf_zero_and_audio_copy() {
    let mut cmd = Command::new("echo");
    append_encode_args(&mut cmd, GridVideoExportQuality::Lossless);
    let args: Vec<String> = cmd
      .get_args()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();
    assert!(args.windows(2).any(|w| w == ["-crf", "0"]));
    assert!(args.windows(2).any(|w| w == ["-c:a", "copy"]));
  }

  #[test]
  fn high_encode_uses_crf_seventeen() {
    let mut cmd = Command::new("echo");
    append_encode_args(&mut cmd, GridVideoExportQuality::High);
    let args: Vec<String> = cmd
      .get_args()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();
    assert!(args.windows(2).any(|w| w == ["-crf", "17"]));
    assert!(args.windows(2).any(|w| w == ["-c:a", "aac"]));
  }
}
