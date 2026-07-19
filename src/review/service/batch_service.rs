//! 批次创建与管理。

use std::path::{Path, PathBuf};

use jwalk::WalkDir;

use crate::review::domain::batch::{BatchStats, ReviewBatch};
use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::sqlite_repository::{ensure_cache_dirs, SqliteReviewRepository};

/// 批次业务服务（借用仓储，不持有连接）。
pub struct BatchService<'a> {
    repo: &'a SqliteReviewRepository,
}

impl<'a> BatchService<'a> {
    pub fn new(repo: &'a SqliteReviewRepository) -> Self {
        Self { repo }
    }

    pub fn repo(&self) -> &SqliteReviewRepository {
        self.repo
    }

    /// 从文件夹扫描图片创建批次。
    pub fn create_from_folder(
        &self,
        name: &str,
        folder: &Path,
        recursive: bool,
    ) -> ReviewResult<i64> {
        ensure_cache_dirs()?;
        let paths = scan_images(folder, recursive)?;
        if paths.is_empty() {
            return Err(ReviewError::EmptyBatch);
        }
        self.repo.create_batch(name, &paths)
    }

    /// 扫描文件夹中的图片路径（不创建批次）。
    pub fn scan_folder(folder: &Path, recursive: bool) -> ReviewResult<Vec<PathBuf>> {
        scan_images(folder, recursive)
    }

    /// 从已有路径列表创建批次（转换队列选中文件）。
    pub fn create_from_paths(&self, name: &str, paths: &[PathBuf]) -> ReviewResult<i64> {
        ensure_cache_dirs()?;
        if paths.is_empty() {
            return Err(ReviewError::EmptyBatch);
        }
        self.repo.create_batch(name, paths)
    }

    pub fn list_batches(&self) -> ReviewResult<Vec<ReviewBatch>> {
        self.repo.list_batches()
    }

    pub fn batch_stats(&self, batch_id: i64) -> ReviewResult<BatchStats> {
        self.repo.batch_stats(batch_id)
    }
}

/// 扫描文件夹中的图片文件（扩展名与导入批次一致）。
pub fn scan_images(folder: &Path, recursive: bool) -> ReviewResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !folder.is_dir() {
        return Err(ReviewError::InvalidPath(folder.to_path_buf()));
    }
    let extensions = ["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif", "gif"];
    if recursive {
        for entry in WalkDir::new(folder).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                push_if_image(&entry.path(), &extensions, &mut out);
            }
        }
    } else {
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                push_if_image(&path, &extensions, &mut out);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn push_if_image(path: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if extensions.contains(&ext.to_ascii_lowercase().as_str()) {
            out.push(path.to_path_buf());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_images_respects_recursive_flag() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("sub");
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.path().join("a.png"), b"x").unwrap();
        fs::write(nested.join("b.jpg"), b"y").unwrap();
        fs::write(dir.path().join("skip.txt"), b"z").unwrap();

        let flat = scan_images(dir.path(), false).unwrap();
        assert_eq!(flat.len(), 1);
        assert!(flat[0].ends_with("a.png"));

        let deep = scan_images(dir.path(), true).unwrap();
        assert_eq!(deep.len(), 2);
    }
}
