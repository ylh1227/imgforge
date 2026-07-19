//! 加载系统中文字体，解决 egui 默认字体无法显示中文的问题。
//!
//! egui/epaint 在解析失败时会 `panic!`（见 `ab_glyph_font_from_font_data`）。
//! Windows 上优先候选是 `msyh.ttc`，部分环境无法被 ab_glyph 解析，
//! 再叠加 `windows_subsystem = "windows"` 就会表现为启动闪退。
//! 因此必须先校验再 `set_fonts`，并优先使用单字体 `.ttf`。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;

/// 安装支持中文的字体（优先使用系统自带字体）。
pub fn install_cjk_fonts(ctx: &egui::Context) {
    for path in system_font_candidates() {
        match try_load_font_data(&path) {
            Ok(data) => {
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert("cjk".to_owned(), Arc::new(data));

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

                // set_fonts 内部仍可能 panic（非法 metrics 等）；捕获后换下一个候选。
                let installed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ctx.set_fonts(fonts);
                }));
                match installed {
                    Ok(()) => {
                        tracing::debug!(font = %path.display(), "installed CJK font");
                        return;
                    }
                    Err(_) => {
                        tracing::warn!(
                            font = %path.display(),
                            "CJK font panic during set_fonts; trying next candidate"
                        );
                    }
                }
            }
            Err(err) => {
                tracing::debug!(font = %path.display(), %err, "skip CJK font candidate");
            }
        }
    }

    tracing::warn!("no usable CJK system font found; Chinese UI text may show as squares");
}

/// 读取并校验字体；TTC 会尝试多个 face index。
fn try_load_font_data(path: &Path) -> Result<egui::FontData, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("empty font file".into());
    }

    let max_index = ttc_face_count(&bytes).unwrap_or(1).min(8);
    let mut last_err = String::from("no font face");
    for index in 0..max_index {
        match ab_glyph::FontRef::try_from_slice_and_index(&bytes, index) {
            Ok(_) => {
                let mut data = egui::FontData::from_owned(bytes);
                data.index = index;
                return Ok(data);
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

/// TrueType Collection 的 face 数量；非 TTC 返回 None。
fn ttc_face_count(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 12 || &bytes[0..4] != b"ttcf" {
        return None;
    }
    let count = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    (count > 0).then_some(count)
}

#[cfg(target_os = "macos")]
fn system_font_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
        PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
        PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc"),
        PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
        PathBuf::from("/Library/Fonts/Arial Unicode.ttf"),
    ]
}

#[cfg(target_os = "windows")]
fn system_font_candidates() -> Vec<PathBuf> {
    // 优先单字体 TTF：避免 TTC 解析失败导致 egui panic 闪退。
    vec![
        PathBuf::from(r"C:\Windows\Fonts\simhei.ttf"),
        PathBuf::from(r"C:\Windows\Fonts\simkai.ttf"),
        PathBuf::from(r"C:\Windows\Fonts\simfang.ttf"),
        PathBuf::from(r"C:\Windows\Fonts\msyh.ttf"),
        PathBuf::from(r"C:\Windows\Fonts\msyh.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\msyhbd.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\simsun.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\msjh.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\malgun.ttf"),
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_font_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
        PathBuf::from("/usr/share/fonts/truetype/arphic/uming.ttc"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttc_header_parses_face_count() {
        let mut bytes = vec![0u8; 12];
        bytes[0..4].copy_from_slice(b"ttcf");
        bytes[8..12].copy_from_slice(&3u32.to_be_bytes());
        assert_eq!(ttc_face_count(&bytes), Some(3));
        assert_eq!(ttc_face_count(b"OTTO...."), None);
    }
}
