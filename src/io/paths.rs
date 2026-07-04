//! 跨平台路径规范化工具。

use std::path::{Component, Path, PathBuf};

use crate::core::error::{AppError, AppResult};

/// 规范化路径（Windows 下去除 `\\?\` 扩展前缀，便于比较）。
pub fn canonicalize(path: &Path) -> PathBuf {
  dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 计算 `path` 相对于 `base` 的相对路径。
pub fn relative_path(base: &Path, path: &Path) -> PathBuf {
  let base = canonicalize(base);
  let path = canonicalize(path);

  if let Ok(rel) = path.strip_prefix(&base) {
    return rel.to_path_buf();
  }

  // 逐组件匹配（处理不同大小写盘符等边缘情况）
  let base_components: Vec<_> = base.components().collect();
  let path_components: Vec<_> = path.components().collect();
  let mut shared = 0usize;
  for (a, b) in base_components.iter().zip(path_components.iter()) {
    if component_eq_ignore_case(a, b) {
      shared += 1;
    } else {
      break;
    }
  }

  if shared == base_components.len() {
    return path_components[shared..].iter().collect();
  }

  path
    .file_name()
    .map(PathBuf::from)
    .unwrap_or_else(|| path.to_path_buf())
}

/// 校验路径不含 `..` 遍历组件。
pub fn ensure_safe_relative(path: &Path) -> AppResult<()> {
  if path
    .components()
    .any(|c| matches!(c, Component::ParentDir))
  {
    return Err(AppError::PathTraversal(path.to_path_buf()));
  }
  Ok(())
}

/// 将模板中的目录占位符规范为 POSIX 风格（避免 Windows `\` 进入文件名）。
pub fn normalize_dir_component(dir: &str) -> String {
  dir.replace('\\', "/")
}

#[cfg(windows)]
fn component_eq_ignore_case(a: &Component<'_>, b: &Component<'_>) -> bool {
  use std::ffi::OsStr;
  match (a, b) {
    (Component::Normal(a), Component::Normal(b)) => {
      a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    }
    (Component::CurDir, Component::CurDir) => true,
    (Component::ParentDir, Component::ParentDir) => true,
    (Component::Prefix(a), Component::Prefix(b)) => a.as_os_str() == b.as_os_str(),
    _ => a.as_os_str() == b.as_os_str(),
  }
}

#[cfg(not(windows))]
fn component_eq_ignore_case(a: &Component<'_>, b: &Component<'_>) -> bool {
  a == b
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn normalize_dir_replaces_backslashes() {
    assert_eq!(normalize_dir_component("a\\b"), "a/b");
  }

  #[test]
  fn relative_path_from_file_name_fallback() {
    let base = Path::new("/photos");
    let file = Path::new("/other/photo.jpg");
    let rel = relative_path(base, file);
    assert_eq!(rel, Path::new("photo.jpg"));
  }
}
