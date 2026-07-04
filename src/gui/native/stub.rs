//! 非 macOS 平台的空实现。

use eframe::Frame;

/// 原生底部工具栏占位高度。
pub const TOOLBAR_HEIGHT: f32 = 0.0;

/// 工具栏按钮动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
  Start,
  Cancel,
  OpenOutput,
}

/// 非 macOS 无原生玻璃工具栏。
pub struct NativeGlassToolbar;

impl NativeGlassToolbar {
  pub fn try_install(_frame: &Frame) -> Option<Self> {
    None
  }

  pub fn is_active(&self) -> bool {
    false
  }

  pub fn sync(&self, _enabled: bool, _running: bool) {}

  pub fn layout(&self) {}

  pub fn drain_actions(&mut self) -> Vec<ToolbarAction> {
    Vec::new()
  }
}
