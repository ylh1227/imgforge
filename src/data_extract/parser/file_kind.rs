//! 文件类型探测。

use std::path::Path;

/// 可解析的 Imatest 结果文件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Csv,
    Json,
    Txt,
    Html,
    Image,
    Unknown,
}

impl FileKind {
    pub fn from_path(path: &Path) -> Self {
        if is_image_file(path) {
            return Self::Image;
        }
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        {
            Some(ext) if ext == "csv" || ext == "tsv" => Self::Csv,
            Some(ext) if ext == "json" => Self::Json,
            Some(ext) if ext == "txt" || ext == "ini" || ext == "log" => Self::Txt,
            Some(ext) if ext == "html" || ext == "htm" => Self::Html,
            _ => Self::Unknown,
        }
    }

    pub fn is_parseable(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

pub fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("tif") | Some("tiff")
    )
}

/// 判断路径是否可能是 Imatest 结果文件。
pub fn is_candidate_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let kind = FileKind::from_path(path);
    if kind.is_parseable() {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.contains("imatest")
        || name.contains("results")
        || name.contains("summary")
        || name.contains("report")
}
