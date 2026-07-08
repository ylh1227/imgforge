//! 图片元数据（纯数据结构 + 轻量读取）。

use std::io::BufReader;
use std::path::Path;

use image::GenericImageView;
use serde::{Deserialize, Serialize};

use crate::review::error::{ReviewError, ReviewResult};

/// 评审属性面板展示的图片元数据。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageMetadata {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_depth: Option<u8>,
    pub color_space: Option<String>,
    pub file_size: Option<u64>,
    pub exif_summary: Option<String>,
    #[serde(default)]
    pub exif_fields: Vec<ExifField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExifField {
    pub label: String,
    pub value: String,
}

impl ImageMetadata {
    pub fn resolution_label(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w} × {h}"),
            _ => "—".into(),
        }
    }

    pub fn file_size_label(&self) -> String {
        self.file_size
            .map(format_bytes)
            .unwrap_or_else(|| "—".into())
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 从磁盘读取基础元数据（不阻塞 UI 时应在线程池中调用）。
pub fn read_image_metadata(path: &Path) -> ReviewResult<ImageMetadata> {
    let file_size = std::fs::metadata(path).ok().map(|m| m.len());
    let img = image::open(path).map_err(|source| ReviewError::ImageDecode {
        path: path.to_path_buf(),
        source,
    })?;
    let (width, height) = img.dimensions();
    let color = img.color();
    let bit_depth = Some(color.bytes_per_pixel() as u8 * 8);
    let color_space = Some(format!("{color:?}"));
    Ok(ImageMetadata {
        width: Some(width),
        height: Some(height),
        bit_depth,
        color_space,
        file_size,
        exif_summary: read_exif_summary(path),
        exif_fields: read_exif_fields(path),
    })
}

fn read_exif_summary(path: &Path) -> Option<String> {
    let fields = read_exif_fields(path);
    let camera = fields
        .iter()
        .find(|f| f.label == "设备型号")
        .map(|f| f.value.clone());
    let iso = fields
        .iter()
        .find(|f| f.label == "ISO")
        .map(|f| format!("ISO {}", f.value));
    let exposure = fields
        .iter()
        .find(|f| f.label == "曝光时间")
        .map(|f| f.value.clone());
    let parts: Vec<String> = [camera, iso, exposure].into_iter().flatten().collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn read_exif_fields(path: &Path) -> Vec<ExifField> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return Vec::new();
    };

    let wanted = [
        (exif::Tag::Make, "制造商"),
        (exif::Tag::Model, "设备型号"),
        (exif::Tag::LensModel, "镜头"),
        (exif::Tag::ISOSpeed, "ISO"),
        (exif::Tag::ExposureTime, "曝光时间"),
        (exif::Tag::FNumber, "光圈"),
        (exif::Tag::FocalLength, "焦距"),
        (exif::Tag::DateTimeOriginal, "拍摄时间"),
        (exif::Tag::Orientation, "方向"),
        (exif::Tag::Software, "软件"),
    ];

    wanted
        .into_iter()
        .filter_map(|(tag, label)| {
            exif.get_field(tag, exif::In::PRIMARY)
                .or_else(|| exif.fields().find(|f| f.tag == tag))
                .map(|field| ExifField {
                    label: label.to_string(),
                    value: field.display_value().with_unit(&exif).to_string(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_scales() {
        assert!(format_bytes(512).contains("B"));
        assert!(format_bytes(2048).contains("KB"));
    }

    #[test]
    fn parses_tiff_exif_make_and_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exif.tiff");
        let make = b"Apple\0";
        let model = b"iPhone 15\0";
        let make_offset = 38u32;
        let model_offset = make_offset + make.len() as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0x010Fu16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&(make.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&make_offset.to_le_bytes());
        bytes.extend_from_slice(&0x0110u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&(model.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&model_offset.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(make);
        bytes.extend_from_slice(model);
        std::fs::write(&path, bytes).unwrap();

        let fields = read_exif_fields(&path);
        assert!(fields
            .iter()
            .any(|f| f.label == "制造商" && f.value.contains("Apple")));
        assert!(fields
            .iter()
            .any(|f| f.label == "设备型号" && f.value.contains("iPhone 15")));
    }
}
