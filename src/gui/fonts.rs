//! 加载系统中文字体，解决 egui 默认字体无法显示中文的问题。

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;

/// 安装支持中文的字体（优先使用系统自带字体）。
pub fn install_cjk_fonts(ctx: &egui::Context) {
    for path in system_font_candidates() {
        if let Ok(bytes) = std::fs::read(&path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "cjk".to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_owned());

            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "cjk".to_owned());

            ctx.set_fonts(fonts);
            tracing::debug!(font = %path.display(), "installed CJK font");
            return;
        }
    }

    tracing::warn!("no CJK system font found; Chinese UI text may show as squares");
}

#[cfg(target_os = "macos")]
fn system_font_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
        PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc"),
        PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
    ]
}

#[cfg(target_os = "windows")]
fn system_font_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\Windows\Fonts\msyh.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\msyhbd.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\simhei.ttf"),
        PathBuf::from(r"C:\Windows\Fonts\simsun.ttc"),
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_font_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
    ]
}
