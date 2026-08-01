//! 场景名前缀命名：`{scene}_{original_stem}{ext}`，幂等、可换场景。

use std::path::{Path, PathBuf};

/// 清理场景名中的非法文件名字符。
pub fn sanitize_scene_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "场景".into();
    }
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => out.push('_'),
            c if c.is_control() => out.push('_'),
            c => out.push(c),
        }
    }
    let collapsed: String = out
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if collapsed.is_empty() {
        "场景".into()
    } else {
        collapsed
    }
}

/// 若 stem 以某个已知场景名 + `_` 开头，剥掉该前缀。
pub fn strip_known_scene_prefix(stem: &str, known_names: &[&str]) -> String {
    let mut names: Vec<&str> = known_names.to_vec();
    // 长名优先，避免短前缀误剥
    names.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
    for name in names {
        let prefix = format!("{}_", sanitize_scene_name(name));
        if let Some(rest) = stem.strip_prefix(&prefix) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    // 也剥「未识别_」
    if let Some(rest) = stem.strip_prefix("未识别_") {
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    stem.to_string()
}

/// 构造目标文件名（不含目录）。
pub fn build_prefixed_filename(
    original_path: &Path,
    scene_name: Option<&str>,
    known_names: &[&str],
) -> String {
    let ext = original_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let stem = original_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let base = strip_known_scene_prefix(stem, known_names);
    match scene_name {
        Some(name) => {
            let safe = sanitize_scene_name(name);
            format!("{safe}_{base}{ext}")
        }
        None => format!("{base}{ext}"),
    }
}

/// 在同目录下生成不冲突的路径（必要时 `__2` 后缀）。
pub fn unique_path_in_dir(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for i in 2..10_000 {
        let name = format!("{stem}__{i}{ext}");
        let p = dir.join(&name);
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}__conflict{ext}"))
}

/// 将场景前缀应用到路径；返回新路径。若无需改名则返回原路径。
pub fn apply_scene_prefix(
    path: &Path,
    scene_name: Option<&str>,
    known_names: &[&str],
) -> std::io::Result<PathBuf> {
    let Some(parent) = path.parent() else {
        return Ok(path.to_path_buf());
    };
    let new_name = build_prefixed_filename(path, scene_name, known_names);
    let current_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if current_name == new_name {
        return Ok(path.to_path_buf());
    }
    let mut dest = parent.join(&new_name);
    if dest.exists() && dest != path {
        dest = unique_path_in_dir(parent, &new_name);
    }
    if dest == path {
        return Ok(path.to_path_buf());
    }
    std::fs::rename(path, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_basic() {
        let p = Path::new("/tmp/IMG_0001.jpg");
        let names = ["夜景", "人像"];
        assert_eq!(
            build_prefixed_filename(p, Some("夜景"), &names),
            "夜景_IMG_0001.jpg"
        );
    }

    #[test]
    fn idempotent_same_scene() {
        let p = Path::new("/tmp/夜景_IMG_0001.jpg");
        let names = ["夜景", "人像"];
        assert_eq!(
            build_prefixed_filename(p, Some("夜景"), &names),
            "夜景_IMG_0001.jpg"
        );
    }

    #[test]
    fn switch_scene() {
        let p = Path::new("/tmp/夜景_IMG_0001.jpg");
        let names = ["夜景", "人像"];
        assert_eq!(
            build_prefixed_filename(p, Some("人像"), &names),
            "人像_IMG_0001.jpg"
        );
    }

    #[test]
    fn sanitize_path_chars() {
        assert_eq!(sanitize_scene_name("夜/景"), "夜_景");
    }
}
