//! 原子文件写入：临时文件 + 重命名，防止崩溃产生损坏文件。

use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::core::error::{AppError, AppResult};
use crate::io::paths;

/// 校验输出路径，防止路径穿越攻击。
pub fn validate_output_path(base: &Path, output: &Path) -> AppResult<()> {
    // 拒绝显式 `..` 组件。
    paths::ensure_safe_relative(output)?;

    // 若输出根目录已存在，且能解析出相对关系，则再次确认相对路径安全。
    // 输出可在不同盘符（Windows），此时 strip_prefix 失败属正常，不视为穿越。
    if base.exists() {
        let base_canon = paths::canonicalize(base);
        if let Some(parent) = output.parent() {
            if parent.exists() {
                let parent_canon = paths::canonicalize(parent);
                if let Ok(rel) = parent_canon.strip_prefix(&base_canon) {
                    paths::ensure_safe_relative(rel)?;
                }
            }
        }
    }

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

    file.write_all(data)
        .await
        .map_err(|e| AppError::io(&tmp_path, e))?;

    file.sync_all()
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
