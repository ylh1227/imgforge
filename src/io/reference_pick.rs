//! 从输入目录挑选亮度匹配参考图；按文件配对同名参考。

use std::path::{Path, PathBuf};

/// 配对参考扩展名优先级（同 stem 时按此顺序选取）。
const PAIRED_EXT_PRIORITY: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// 参考图允许的扩展名（与 GUI 选择过滤器一致）。
pub fn is_reference_image_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp"
    )
}

/// 为源文件查找同目录同名参考图（jpg/jpeg/png/webp），排除源文件自身。
///
/// 扩展名优先级：`jpg` → `jpeg` → `png` → `webp`。
pub fn find_paired_reference(source: &Path) -> Option<PathBuf> {
    let parent = source.parent()?;
    let stem = source.file_stem()?.to_str()?;
    for ext in PAIRED_EXT_PRIORITY {
        let candidate = parent.join(format!("{stem}.{ext}"));
        if !candidate.is_file() {
            continue;
        }
        if paths_same_file(source, &candidate) {
            continue;
        }
        return Some(candidate);
    }
    None
}

fn paths_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}

fn stem_looks_like_reference(stem: &str) -> bool {
    let s = stem.to_ascii_lowercase();
    s.contains("ref") || s.contains("reference") || stem.contains("参考")
}

/// 从输入目录挑选参考图。
///
/// 优先级：文件名含 `ref` / `reference` / `参考`（不区分大小写，除中文）→
/// 否则按路径字符串排序后的第一张 jpg/jpeg/png/webp。
pub fn pick_reference_from_input(input_dir: &Path, recursive: bool) -> Option<PathBuf> {
    if !input_dir.is_dir() {
        return None;
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_images(input_dir, recursive, &mut candidates);
    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        a.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&b.to_string_lossy().to_ascii_lowercase())
    });

    if let Some(pref) = candidates.iter().find(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(stem_looks_like_reference)
    }) {
        return Some(pref.clone());
    }

    candidates.into_iter().next()
}

fn collect_images(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_images(&path, true, out);
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if is_reference_image_ext(ext) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "imgforge_ref_pick_{nanos}_{}",
            std::process::id()
        ));
        // 再拼一层随机，避免并行同 nanos 冲突
        let dir = dir.join(format!("{:x}", nanos.wrapping_mul(0x9e37_79b9)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn prefers_filename_with_ref() {
        let dir = temp_dir();
        touch(&dir.join("a.jpg"));
        touch(&dir.join("scene_ref.png"));
        touch(&dir.join("b.webp"));
        let picked = pick_reference_from_input(&dir, false).unwrap();
        assert_eq!(picked.file_name().unwrap(), "scene_ref.png");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_sorted_first() {
        let dir = temp_dir();
        touch(&dir.join("z.jpg"));
        touch(&dir.join("a.png"));
        let picked = pick_reference_from_input(&dir, false).unwrap();
        assert_eq!(picked.file_name().unwrap(), "a.png");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_dir_returns_none() {
        let dir = temp_dir();
        assert!(pick_reference_from_input(&dir, false).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recursive_finds_nested_ref() {
        let dir = temp_dir();
        let nested = dir.join("sub");
        fs::create_dir_all(&nested).unwrap();
        touch(&dir.join("top.jpg"));
        touch(&nested.join("reference_shot.jpeg"));
        let picked = pick_reference_from_input(&dir, true).unwrap();
        assert_eq!(picked.file_name().unwrap(), "reference_shot.jpeg");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn paired_prefers_jpg_over_png() {
        let dir = temp_dir();
        let raw = dir.join("shot.CR2");
        touch(&raw);
        touch(&dir.join("shot.png"));
        touch(&dir.join("shot.jpg"));
        let paired = find_paired_reference(&raw).unwrap();
        assert_eq!(paired.file_name().unwrap(), "shot.jpg");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn paired_skips_self_when_source_is_jpg() {
        let dir = temp_dir();
        let jpg = dir.join("a.jpg");
        touch(&jpg);
        assert!(find_paired_reference(&jpg).is_none());
        touch(&dir.join("a.png"));
        let paired = find_paired_reference(&jpg).unwrap();
        assert_eq!(paired.file_name().unwrap(), "a.png");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn paired_missing_returns_none() {
        let dir = temp_dir();
        let raw = dir.join("lonely.NEF");
        touch(&raw);
        assert!(find_paired_reference(&raw).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
