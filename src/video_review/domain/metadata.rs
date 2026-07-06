//! 视频元数据（ffprobe 解析结果）。

#[derive(Debug, Clone, Default)]
pub struct VideoMetadata {
  pub duration_ms: u64,
  pub fps: f32,
  pub width: u32,
  pub height: u32,
  pub video_codec: String,
  pub audio_codec: Option<String>,
  pub bitrate_kbps: Option<u32>,
}

impl VideoMetadata {
  pub fn duration_label(&self) -> String {
    let total = self.duration_ms / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
      format!("{h:02}:{m:02}:{s:02}")
    } else {
      format!("{m:02}:{s:02}")
    }
  }

  pub fn resolution_label(&self) -> String {
    if self.width > 0 && self.height > 0 {
      format!("{}×{}", self.width, self.height)
    } else {
      "—".into()
    }
  }
}
