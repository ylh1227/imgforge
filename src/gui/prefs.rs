//! GUI 用户偏好：转换预设与任务历史（`~/.imgforge/gui_prefs.json`）。

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::types::{ImageFormat, MetadataPolicy, Quality, ResizeOptions};

const MAX_HISTORY: usize = 12;
const MAX_PRESETS: usize = 20;

fn prefs_path() -> PathBuf {
  #[cfg(target_os = "windows")]
  {
    let base = std::env::var("APPDATA")
      .map(PathBuf::from)
      .unwrap_or_else(|_| PathBuf::from("."));
    base.join("imgforge").join("gui_prefs.json")
  }
  #[cfg(not(target_os = "windows"))]
  {
    std::env::var("HOME")
      .map(|h| PathBuf::from(h).join(".imgforge").join("gui_prefs.json"))
      .unwrap_or_else(|_| PathBuf::from("gui_prefs.json"))
  }
}

/// 可保存/恢复的转换参数快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertPresetSnapshot {
  pub format: ImageFormat,
  pub quality: u8,
  pub resize: ResizeOptions,
  pub recursive: bool,
  pub preserve_structure: bool,
  pub overwrite: bool,
  pub strip_metadata: bool,
  pub bayer_only: bool,
  pub rename_template: String,
  pub target_max_bytes: Option<u64>,
  pub use_target_max_bytes: bool,
}

impl Default for ConvertPresetSnapshot {
  fn default() -> Self {
    Self {
      format: ImageFormat::WebP,
      quality: Quality::DEFAULT.value(),
      resize: ResizeOptions {
        width: None,
        height: None,
        mode: crate::core::types::ResizeMode::Fit,
      },
      recursive: true,
      preserve_structure: true,
      overwrite: false,
      strip_metadata: false,
      bayer_only: false,
      rename_template: String::new(),
      target_max_bytes: None,
      use_target_max_bytes: false,
    }
  }
}

/// 单次转换任务历史记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryEntry {
  pub finished_at_unix: u64,
  pub input_dir: String,
  pub output_dir: String,
  pub successes: usize,
  pub failures: usize,
  pub total: usize,
  pub elapsed_ms: u64,
  pub snapshot: ConvertPresetSnapshot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuiPrefs {
  pub presets: Vec<NamedPreset>,
  pub history: Vec<TaskHistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedPreset {
  pub name: String,
  pub snapshot: ConvertPresetSnapshot,
}

impl GuiPrefs {
  pub fn load() -> Self {
    let path = prefs_path();
    if !path.exists() {
      return Self::default();
    }
    fs::read_to_string(&path)
      .ok()
      .and_then(|s| serde_json::from_str(&s).ok())
      .unwrap_or_default()
  }

  pub fn save(&self) -> std::io::Result<()> {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(self).map_err(|e| {
      std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;
    fs::write(path, json)
  }

  pub fn upsert_preset(&mut self, name: String, snapshot: ConvertPresetSnapshot) {
    if let Some(p) = self.presets.iter_mut().find(|p| p.name == name) {
      p.snapshot = snapshot;
    } else {
      self.presets.push(NamedPreset { name, snapshot });
      if self.presets.len() > MAX_PRESETS {
        self.presets.remove(0);
      }
    }
  }

  pub fn delete_preset(&mut self, name: &str) {
    self.presets.retain(|p| p.name != name);
  }

  pub fn push_history(&mut self, entry: TaskHistoryEntry) {
    self.history.insert(0, entry);
    self.history.truncate(MAX_HISTORY);
  }
}

pub fn now_unix() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0)
}
