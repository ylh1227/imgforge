//! 原子文件写入：临时文件 + 重命名，防止崩溃产生损坏文件。

use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::core::error::{AppError, AppResult};
use crate::io::paths;

/// 校验输出路径，防止路径穿越攻击。
pub fn validate_output_path(base: &Path, output: &Path) -> AppResult<()> {
  if output
    .components()
    .any(|c| matches!(c, std::path::Component::ParentDir))
  {
    return Err(AppError::PathTraversal(output.to_path_buf()));
  }

  // 输出目录可以独立于输入目录（跨盘符在 Windows 上很常见）
  let _base = paths::canonicalize(base);
  let _parent = output
    .parent()
    .map(paths::canonicalize)
    .unwrap_or_else(|| PathBuf::from("."));

  Ok(())
}

/// 异步原子写入：先写 `.tmp` 再 `rename`（Windows 下先删除目标文件）。
pub async fn atomic_write(path: &Path, data: &[u8]) -> AppResult<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)
      .await
      .map_err(|e| AppError::io(parent, e))?;
  }

  let tmp_path = temp_path(path);
  let mut file = fs::File::create(&tmp_path)
    .await
    .map_err(|e| AppError::io(&tmp_path, e))?;

  file
    .write_all(data)
    .await
    .map_err(|e| AppError::io(&tmp_path, e))?;

  file
    .sync_all()
    .await
    .map_err(|e| AppError::io(&tmp_path, e))?;

  drop(file);

  replace_file(&tmp_path, path).await
}

async fn replace_file(tmp_path: &Path, dest_path: &Path) -> AppResult<()> {
  // Windows 上 rename 无法覆盖已存在文件，需先删除目标
  if dest_path.exists() {
    fs::remove_file(dest_path)
      .await
      .map_err(|e| AppError::io(dest_path, e))?;
  }

  match fs::rename(tmp_path, dest_path).await {
    Ok(()) => Ok(()),
    Err(e) => {
      let _ = fs::remove_file(tmp_path).await;
      Err(AppError::io(dest_path, e))
    }
  }
}

fn temp_path(path: &Path) -> PathBuf {
  let file_name = path
    .file_name()
    .map(|n| n.to_string_lossy().to_string())
    .unwrap_or_else(|| "output".to_string());
  path.with_file_name(format!("{file_name}.tmp"))
}
