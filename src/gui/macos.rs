//! macOS 系统偏好（无障碍、对比度），供 Liquid Glass 主题回退使用。

/// 与 macOS「辅助功能」相关的显示偏好。
#[derive(Debug, Clone, Copy, Default)]
pub struct AccessibilityPrefs {
    /// 降低透明度（系统设置 → 辅助功能 → 显示）
    pub reduce_transparency: bool,
    /// 增强对比度
    pub increase_contrast: bool,
}

/// 读取当前 macOS 辅助功能偏好；非 macOS 平台返回默认值。
pub fn accessibility_prefs() -> AccessibilityPrefs {
    #[cfg(target_os = "macos")]
    {
        AccessibilityPrefs {
            reduce_transparency: global_defaults_bool("AppleReduceTransparency"),
            increase_contrast: global_defaults_bool("AppleContrastIncrease"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        AccessibilityPrefs::default()
    }
}

#[cfg(target_os = "macos")]
fn global_defaults_bool(key: &str) -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", key])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "1")
        .unwrap_or(false)
}
