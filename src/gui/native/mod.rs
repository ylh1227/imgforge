//! 平台原生 UI 层（macOS AppKit Liquid Glass）。

#[cfg(target_os = "macos")]
mod macos_glass;

#[cfg(target_os = "macos")]
pub use macos_glass::{NativeGlassToolbar, ToolbarAction, TOOLBAR_HEIGHT};

#[cfg(not(target_os = "macos"))]
mod stub;

#[cfg(not(target_os = "macos"))]
pub use stub::{NativeGlassToolbar, ToolbarAction, TOOLBAR_HEIGHT};
