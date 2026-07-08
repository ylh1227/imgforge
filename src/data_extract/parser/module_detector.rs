//! 根据文件名/目录名/内容识别 Imatest 测试域。

use std::path::Path;

use crate::data_extract::domain::ImatestModule;

/// 从路径推断测试模块。
pub fn detect_module_from_path(path: &Path) -> Option<ImatestModule> {
    let mut haystack = String::new();
    if let Some(parent) = path.parent() {
        haystack.push_str(&parent.to_string_lossy().to_ascii_lowercase());
        haystack.push(' ');
    }
    haystack.push_str(&path.to_string_lossy().to_ascii_lowercase());

    detect_module_from_text(&haystack)
}

/// 从文本内容推断测试模块。
pub fn detect_module_from_text(text: &str) -> Option<ImatestModule> {
    let lower = text.to_ascii_lowercase();
    let mut best: Option<(ImatestModule, usize)> = None;

    for module in ImatestModule::ALL {
        let score = module
            .keywords()
            .iter()
            .map(|kw| keyword_score(&lower, kw))
            .sum::<usize>();
        if score > 0 {
            match best {
                None => best = Some((module, score)),
                Some((_, prev)) if score > prev => best = Some((module, score)),
                _ => {}
            }
        }
    }

    best.map(|(m, _)| m)
}

fn keyword_score(haystack: &str, keyword: &str) -> usize {
    if haystack.contains(keyword) {
        keyword.len() + 2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_mtf_from_filename() {
        let p = Path::new("/results/esfr_mtf_center.csv");
        assert_eq!(detect_module_from_path(p), Some(ImatestModule::Mtf));
    }

    #[test]
    fn detect_distortion_from_chinese_path() {
        let p = Path::new("/测试课堂第一讲/畸变Distortion/summary.csv");
        assert_eq!(detect_module_from_path(p), Some(ImatestModule::Distortion));
    }
}
