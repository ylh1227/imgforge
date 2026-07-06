//! 多视频宫格拼接视频导出（ffmpeg xstack）。

use std::path::PathBuf;
use std::process::Command;

use crate::video_review::domain::VideoItem;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::contact_sheet::grid_dimensions;

pub const DEFAULT_CELL_WIDTH: u32 = 640;
pub const DEFAULT_CELL_HEIGHT: u32 = 360;
pub const DEFAULT_CLIP_DURATION_MS: u64 = 10_000;
pub const MIN_CLIP_DURATION_MS: u64 = 500;

#[derive(Debug, Clone)]
pub struct GridVideoExportRequest {
  pub videos: Vec<VideoItem>,
  pub start_time_ms: u64,
  pub duration_ms: u64,
  pub dest: PathBuf,
  pub cell_width: u32,
  pub cell_height: u32,
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
      cell_width: DEFAULT_CELL_WIDTH,
      cell_height: DEFAULT_CELL_HEIGHT,
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
    let cell_w = req.cell_width.max(160);
    let cell_h = req.cell_height.max(90);
    let out_w = cols as u32 * cell_w;
    let out_h = rows as u32 * cell_h;

    let filter = build_filter_complex(count, cols, cell_w, cell_h);
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
    cmd.args([
      "-c:v",
      "libx264",
      "-preset",
      "fast",
      "-crf",
      "23",
      "-c:a",
      "aac",
      "-b:a",
      "128k",
      "-movflags",
      "+faststart",
    ]);
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
    })
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

pub fn build_filter_complex(count: usize, cols: usize, cell_w: u32, cell_h: u32) -> String {
  let scale = format!(
    "scale={cell_w}:{cell_h}:force_original_aspect_ratio=decrease,pad={cell_w}:{cell_h}:(ow-iw)/2:(oh-ih)/2,setsar=1,setpts=PTS-STARTPTS"
  );
  let mut parts = Vec::with_capacity(count + 1);
  for i in 0..count {
    parts.push(format!("[{i}:v]{scale}[v{i}]"));
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
    VideoItem {
      id,
      batch_id: 1,
      file_path: PathBuf::from(format!("/tmp/v{id}.mp4")),
      status: ReviewStatus::Pending,
      remark: None,
      thumbnail_path: None,
      duration_ms,
      fps: 24.0,
      width: 1920,
      height: 1080,
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
  fn xstack_layout_two_by_two() {
    assert_eq!(build_xstack_layout(4, 2, 640, 360), "0_0|640_0|0_360|640_360");
  }

  #[test]
  fn filter_complex_uses_xstack() {
    let f = build_filter_complex(2, 2, 640, 360);
    assert!(f.contains("xstack=inputs=2"));
    assert!(f.contains("[0:v]"));
    assert!(f.contains("[1:v]"));
    assert!(f.contains("[outv]"));
  }
}
