//! 批量重命名模板解析（feature: rename）。

use std::path::Path;

use crate::core::error::{AppError, AppResult};
use crate::io::paths;

/// 重命名模板上下文。
pub struct RenameContext {
  pub stem: String,
  pub name: String,
  pub ext: String,
  pub dir: String,
  pub index: usize,
  pub width: Option<u32>,
  pub height: Option<u32>,
}

/// 将模板中的占位符替换为实际值。
///
/// 支持占位符：`{stem}` `{name}` `{ext}` `{dir}` `{index}` `{width}` `{height}`
pub fn apply_rename_template(template: &str, ctx: &RenameContext) -> AppResult<String> {
  if template.is_empty() {
    return Err(AppError::Config("rename template is empty".into()));
  }

  let mut result = template.to_string();
  result = result.replace("{stem}", &ctx.stem);
  result = result.replace("{name}", &ctx.name);
  result = result.replace("{ext}", &ctx.ext);
  result = result.replace("{dir}", &ctx.dir);
  result = result.replace("{index}", &ctx.index.to_string());
  result = result.replace(
    "{width}",
    &ctx.width.map(|w| w.to_string()).unwrap_or_default(),
  );
  result = result.replace(
    "{height}",
    &ctx.height.map(|h| h.to_string()).unwrap_or_default(),
  );

  if result.contains('{') {
    return Err(AppError::Config(format!(
      "rename template contains unknown placeholders: {template}"
    )));
  }

  if result.contains('/') || result.contains('\\') {
    return Err(AppError::Config(
      "rename template must not contain path separators".into(),
    ));
  }

  Ok(result)
}

/// 从源路径提取重命名上下文。
pub fn context_from_path(
  input_root: &Path,
  source: &Path,
  index: usize,
  width: Option<u32>,
  height: Option<u32>,
) -> RenameContext {
  let relative = paths::relative_path(input_root, source);
  let stem = relative
    .file_stem()
    .map(|s| s.to_string_lossy().to_string())
    .unwrap_or_else(|| "output".to_string());
  let name = relative
    .file_name()
    .map(|s| s.to_string_lossy().to_string())
    .unwrap_or_else(|| "output".to_string());
  let ext = source
    .extension()
    .map(|e| e.to_string_lossy().to_string())
    .unwrap_or_default();
  let dir = relative
    .parent()
    .map(|p| paths::normalize_dir_component(&p.to_string_lossy()))
    .unwrap_or_default();

  RenameContext {
    stem,
    name,
    ext,
    dir,
    index,
    width,
    height,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn template_replaces_placeholders() {
    let ctx = RenameContext {
      stem: "photo".into(),
      name: "photo.png".into(),
      ext: "png".into(),
      dir: "album".into(),
      index: 2,
      width: Some(800),
      height: Some(600),
    };
    let out = apply_rename_template("{dir}_{stem}_{width}x{height}", &ctx).unwrap();
    assert_eq!(out, "album_photo_800x600");
  }

  #[test]
  fn rejects_unknown_placeholder() {
    let ctx = RenameContext {
      stem: "a".into(),
      name: "a.jpg".into(),
      ext: "jpg".into(),
      dir: "".into(),
      index: 0,
      width: None,
      height: None,
    };
    assert!(apply_rename_template("{unknown}", &ctx).is_err());
  }
}
