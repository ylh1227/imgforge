//! 目录扫描。

use std::path::{Path, PathBuf};

use jwalk::WalkDir;

use crate::data_extract::parser::file_kind::is_candidate_file;

/// 扫描目录中的 Imatest 候选结果文件。
pub fn scan_directory(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if root.is_file() {
        if is_candidate_file(root) {
            files.push(root.to_path_buf());
        }
        return files;
    }

    for entry in WalkDir::new(root).follow_links(false) {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.is_file() && is_candidate_file(&path) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn scan_finds_csv_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("mtf_results.csv"), "a,b\n1,2").unwrap();
        fs::write(dir.path().join("readme.md"), "ignore").unwrap();
        let found = scan_directory(dir.path());
        assert_eq!(found.len(), 1);
    }
}
