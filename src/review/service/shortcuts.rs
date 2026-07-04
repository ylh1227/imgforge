//! 快捷键配置持久化（数据层，不含 egui 依赖）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::review::error::ReviewResult;
use crate::review::storage::shortcuts_path;

/// 可绑定快捷键的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShortcutAction {
  PrevImage,
  NextImage,
  StatusPending,
  StatusApproved,
  StatusNeedsFix,
  StatusRejected,
  FitWindow,
  ActualSize,
  UndoAnnotation,
}

impl ShortcutAction {
  pub fn default_bindings() -> HashMap<Self, String> {
    HashMap::from([
      (Self::PrevImage, "A,Left".into()),
      (Self::NextImage, "D,Right".into()),
      (Self::StatusPending, "0".into()),
      (Self::StatusApproved, "1".into()),
      (Self::StatusNeedsFix, "2".into()),
      (Self::StatusRejected, "3".into()),
      (Self::FitWindow, "Ctrl+0".into()),
      (Self::ActualSize, "Ctrl+1".into()),
      (Self::UndoAnnotation, "Ctrl+Z".into()),
    ])
  }

  pub fn label(self) -> &'static str {
    match self {
      Self::PrevImage => "上一张",
      Self::NextImage => "下一张",
      Self::StatusPending => "未评审",
      Self::StatusApproved => "通过",
      Self::StatusNeedsFix => "待修正",
      Self::StatusRejected => "驳回",
      Self::FitWindow => "适配窗口",
      Self::ActualSize => "1:1 原始比例",
      Self::UndoAnnotation => "撤销标注",
    }
  }

  pub fn binding_keys(&self, config: &ShortcutConfig) -> Vec<String> {
    config
      .bindings
      .get(self)
      .map(|s| s.split(',').map(|p| p.trim().to_string()).collect())
      .unwrap_or_default()
  }
}

/// 快捷键配置（持久化到本地 JSON）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutConfig {
  pub bindings: HashMap<ShortcutAction, String>,
}

impl Default for ShortcutConfig {
  fn default() -> Self {
    Self {
      bindings: ShortcutAction::default_bindings(),
    }
  }
}

impl ShortcutConfig {
  pub fn load() -> ReviewResult<Self> {
    let path = shortcuts_path()?;
    if !path.exists() {
      return Ok(Self::default());
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).map_err(Into::into)
  }

  pub fn save(&self) -> ReviewResult<()> {
    let path = shortcuts_path()?;
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
    Ok(())
  }
}

pub fn save_custom_binding(action: ShortcutAction, binding: &str) -> ReviewResult<()> {
  let mut cfg = ShortcutConfig::load()?;
  cfg.bindings.insert(action, binding.to_string());
  cfg.save()
}
