//! 数据库自动备份与恢复（保留最近 N 个版本）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::paths::{app_data_dir, database_path};

const MAX_BACKUPS: usize = 5;

pub fn backup_dir() -> ReviewResult<PathBuf> {
  Ok(app_data_dir()?.join("backups"))
}

/// 复制当前数据库到备份目录，超出数量时删除最旧备份。
pub fn create_backup() -> ReviewResult<PathBuf> {
  let src = database_path()?;
  if !src.exists() {
    return Err(ReviewError::Message("数据库文件不存在，跳过备份".into()));
  }
  let dir = backup_dir()?;
  fs::create_dir_all(&dir)?;
  let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
  let dest = dir.join(format!("imgforge_{stamp}.db"));
  fs::copy(&src, &dest)?;
  prune_old_backups(&dir)?;
  tracing::info!(path = %dest.display(), "review database backup created");
  Ok(dest)
}

pub fn list_backups() -> ReviewResult<Vec<PathBuf>> {
  let dir = backup_dir()?;
  if !dir.exists() {
    return Ok(Vec::new());
  }
  let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
    .filter_map(|e| e.ok())
    .map(|e| e.path())
    .filter(|p| p.extension().is_some_and(|ext| ext == "db"))
    .collect();
  files.sort_by_key(|p| {
    fs::metadata(p)
      .and_then(|m| m.modified())
      .unwrap_or(SystemTime::UNIX_EPOCH)
  });
  files.reverse();
  Ok(files)
}

pub fn restore_backup(backup: &Path) -> ReviewResult<()> {
  if !backup.exists() {
    return Err(ReviewError::InvalidPath(backup.to_path_buf()));
  }
  let db = database_path()?;
  let _ = create_backup();
  fs::copy(backup, &db)?;
  tracing::warn!(path = %backup.display(), "review database restored from backup");
  Ok(())
}

fn prune_old_backups(dir: &Path) -> ReviewResult<()> {
  let mut backups = list_backups()?;
  while backups.len() > MAX_BACKUPS {
    if let Some(old) = backups.pop() {
      let _ = fs::remove_file(old);
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn backup_dir_under_app_data() {
    let dir = backup_dir().unwrap();
    assert!(dir.to_string_lossy().contains("imgforge"));
  }
}
