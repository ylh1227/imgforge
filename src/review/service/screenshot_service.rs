//! 图片评审批量截图导出。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::types::{ImageFormat, Quality};
use crate::processing::backends::native_backend::encode_dynamic_image;
use crate::review::domain::{burn_annotations_onto, NormRect};
use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::SqliteReviewRepository;
use crate::ui::progress::ProgressReporter;
use crate::video_review::service::contact_sheet::ContactSheetService;
use crate::video_review::service::screenshot_service::{
    render_filename, unique_output_path, ScreenshotFormat,
};

/// 框选过小视为「未框选」（整图导出）。
const MIN_ROI_SPAN: f32 = 0.005;

#[derive(Debug, Clone)]
pub struct BatchImageScreenshotRequest {
    pub items: Vec<(i64, PathBuf)>,
    pub output_dir: PathBuf,
    pub include_annotations: bool,
    pub format: ScreenshotFormat,
    pub quality: u8,
    pub naming_template: String,
    pub write_csv_manifest: bool,
    pub write_json_manifest: bool,
    pub write_contact_sheet: bool,
    /// 归一化裁切区域；`None` 或过小 → 整图。
    pub crop: Option<NormRect>,
}

impl BatchImageScreenshotRequest {
    pub fn new(items: Vec<(i64, PathBuf)>, output_dir: PathBuf) -> Self {
        Self {
            items,
            output_dir,
            include_annotations: false,
            format: ScreenshotFormat::Jpeg,
            quality: 85,
            naming_template: "{index}_{filename}.{ext}".into(),
            write_csv_manifest: true,
            write_json_manifest: false,
            write_contact_sheet: false,
            crop: None,
        }
    }

    /// 从纯文件路径构建请求（不进评审库，强制不烧标注）。
    pub fn from_paths(paths: Vec<PathBuf>, output_dir: PathBuf) -> Self {
        Self::new(paths.into_iter().map(|p| (0, p)).collect(), output_dir)
    }

    pub fn effective_crop(&self) -> Option<NormRect> {
        self.crop.filter(is_meaningful_roi)
    }
}

/// ROI 是否足够大，值得裁切。
pub fn is_meaningful_roi(roi: &NormRect) -> bool {
    (roi.x1 - roi.x0).abs() > MIN_ROI_SPAN && (roi.y1 - roi.y0).abs() > MIN_ROI_SPAN
}

pub fn format_roi_label(crop: Option<NormRect>) -> String {
    match crop.filter(is_meaningful_roi) {
        Some(r) => format!(
            "roi={:.2},{:.2}–{:.2},{:.2}",
            r.x0.min(r.x1),
            r.y0.min(r.y1),
            r.x0.max(r.x1),
            r.y0.max(r.y1)
        ),
        None => "full".into(),
    }
}

/// 按归一化矩形裁切；坐标夹紧到图像边界。
pub fn apply_norm_crop(rgba: &image::RgbaImage, roi: NormRect) -> ReviewResult<image::RgbaImage> {
    let w = rgba.width();
    let h = rgba.height();
    if w == 0 || h == 0 {
        return Err(ReviewError::Message("图像尺寸无效".into()));
    }

    let x0f = roi.x0.min(roi.x1).clamp(0.0, 1.0);
    let y0f = roi.y0.min(roi.y1).clamp(0.0, 1.0);
    let x1f = roi.x0.max(roi.x1).clamp(0.0, 1.0);
    let y1f = roi.y0.max(roi.y1).clamp(0.0, 1.0);

    let mut x0 = (x0f * w as f32).round() as u32;
    let mut y0 = (y0f * h as f32).round() as u32;
    let mut x1 = (x1f * w as f32).round() as u32;
    let mut y1 = (y1f * h as f32).round() as u32;

    x0 = x0.min(w.saturating_sub(1));
    y0 = y0.min(h.saturating_sub(1));
    x1 = x1.min(w).max(x0.saturating_add(1));
    y1 = y1.min(h).max(y0.saturating_add(1));

    let crop_w = x1.saturating_sub(x0);
    let crop_h = y1.saturating_sub(y0);
    if crop_w < 1 || crop_h < 1 {
        return Err(ReviewError::Message("裁切区域无效".into()));
    }

    Ok(image::imageops::crop_imm(rgba, x0, y0, crop_w, crop_h).to_image())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageScreenshotManifestEntry {
    pub index: usize,
    pub image_id: i64,
    pub source_path: String,
    pub output_path: String,
    pub include_annotations: bool,
    #[serde(default)]
    pub crop: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchImageScreenshotResult {
    pub requested: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub output_files: Vec<PathBuf>,
    pub manifest_entries: Vec<ImageScreenshotManifestEntry>,
    pub csv_manifest: Option<PathBuf>,
    pub json_manifest: Option<PathBuf>,
    pub contact_sheets: Vec<PathBuf>,
}

pub struct BatchImageScreenshotService;

impl BatchImageScreenshotService {
    pub fn export(
        repo: &SqliteReviewRepository,
        request: &BatchImageScreenshotRequest,
        progress: Option<&dyn ProgressReporter>,
    ) -> ReviewResult<BatchImageScreenshotResult> {
        export_inner(Some(repo), request, progress)
    }

    /// 纯路径导出：不打开评审库、不烧标注。
    pub fn export_paths(
        request: &BatchImageScreenshotRequest,
        progress: Option<&dyn ProgressReporter>,
    ) -> ReviewResult<BatchImageScreenshotResult> {
        let mut request = request.clone();
        request.include_annotations = false;
        export_inner(None, &request, progress)
    }
}

fn export_inner(
    repo: Option<&SqliteReviewRepository>,
    request: &BatchImageScreenshotRequest,
    progress: Option<&dyn ProgressReporter>,
) -> ReviewResult<BatchImageScreenshotResult> {
    if request.items.is_empty() {
        return Err(ReviewError::Message("没有可导出的图片".into()));
    }
    if request.include_annotations && repo.is_none() {
        return Err(ReviewError::Message(
            "烧录标注需要评审库，请使用批次内批量截图".into(),
        ));
    }
    fs::create_dir_all(&request.output_dir)?;

    if let Some(p) = progress {
        p.set_total(request.items.len());
    }

    let crop_label = format_roi_label(request.crop);
    let mut result = BatchImageScreenshotResult {
        requested: request.items.len(),
        ..Default::default()
    };

    for (index, (image_id, source)) in request.items.iter().enumerate() {
        if let Some(p) = progress {
            p.set_current_label(
                source
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .as_str(),
            );
        }
        let entry = export_one(repo, request, *image_id, source, index + 1, &crop_label)?;
        if entry.success {
            result.succeeded += 1;
            result.output_files.push(PathBuf::from(&entry.output_path));
        } else {
            result.failed += 1;
        }
        result.manifest_entries.push(entry);
        if let Some(p) = progress {
            p.inc(None);
        }
    }

    if request.write_csv_manifest {
        let path = request.output_dir.join("image_screenshots.csv");
        write_csv_manifest(&path, &result.manifest_entries)?;
        result.csv_manifest = Some(path);
    }
    if request.write_json_manifest {
        let path = request.output_dir.join("image_screenshots.json");
        let json = serde_json::to_string_pretty(&result.manifest_entries)?;
        fs::write(&path, json)?;
        result.json_manifest = Some(path);
    }
    if request.write_contact_sheet {
        let items: Vec<(PathBuf, String)> = result
            .manifest_entries
            .iter()
            .filter(|e| e.success)
            .map(|e| {
                let source = PathBuf::from(&e.source_path);
                let filename = source
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("image");
                let label = format!("{:03}_{filename}", e.index);
                (PathBuf::from(&e.output_path), label)
            })
            .collect();
        if items.len() >= 2 {
            if let Ok(pages) = ContactSheetService::export_image_index_pages(
                &items,
                &request.output_dir,
                "image_screenshots_index",
                320,
            ) {
                result.contact_sheets = pages.into_iter().map(|p| p.dest).collect();
            }
        }
    }

    if let Some(p) = progress {
        p.finish();
    }

    Ok(result)
}

fn export_one(
    repo: Option<&SqliteReviewRepository>,
    request: &BatchImageScreenshotRequest,
    image_id: i64,
    source: &Path,
    index: usize,
    crop_label: &str,
) -> ReviewResult<ImageScreenshotManifestEntry> {
    let filename = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext = request.format.extension();
    let base_name = render_filename(&request.naming_template, index, filename, "", "", "", ext);
    let dest = unique_output_path(&request.output_dir, &base_name);

    let mut entry = ImageScreenshotManifestEntry {
        index,
        image_id,
        source_path: source.display().to_string(),
        output_path: dest.display().to_string(),
        include_annotations: request.include_annotations,
        crop: crop_label.to_string(),
        success: false,
        error: None,
    };

    match export_image_file(repo, source, &dest, image_id, request) {
        Ok(()) => entry.success = true,
        Err(e) => entry.error = Some(e.to_string()),
    }

    Ok(entry)
}

fn export_image_file(
    repo: Option<&SqliteReviewRepository>,
    source: &Path,
    dest: &Path,
    image_id: i64,
    request: &BatchImageScreenshotRequest,
) -> ReviewResult<()> {
    let img = image::open(source).map_err(|source_err| ReviewError::ImageDecode {
        path: source.to_path_buf(),
        source: source_err,
    })?;
    let mut rgba = img.to_rgba8();
    if request.include_annotations {
        let Some(repo) = repo else {
            return Err(ReviewError::Message("烧录标注需要评审库".into()));
        };
        let annotations = repo.list_annotations(image_id)?;
        burn_annotations_onto(&mut rgba, &annotations);
    }

    if let Some(roi) = request.effective_crop() {
        rgba = apply_norm_crop(&rgba, roi)?;
    }

    export_rgba_plain(&rgba, dest, request.format, request.quality)
}

fn export_rgba_plain(
    rgba: &image::RgbaImage,
    dest: &Path,
    format: ScreenshotFormat,
    quality: u8,
) -> ReviewResult<()> {
    let target_format = match format {
        ScreenshotFormat::Jpeg => ImageFormat::Jpeg,
        ScreenshotFormat::Png => ImageFormat::Png,
    };
    let encoded = encode_dynamic_image(
        &image::DynamicImage::ImageRgba8(rgba.clone()),
        target_format,
        Quality::new(quality).unwrap_or(Quality::DEFAULT),
    )
    .map_err(|e| ReviewError::Message(e.to_string()))?;
    fs::write(dest, encoded)?;
    Ok(())
}

fn write_csv_manifest(path: &Path, entries: &[ImageScreenshotManifestEntry]) -> ReviewResult<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "index",
        "image_id",
        "source_path",
        "output_path",
        "include_annotations",
        "crop",
        "success",
        "error",
    ])?;
    for entry in entries {
        wtr.write_record([
            entry.index.to_string(),
            entry.image_id.to_string(),
            entry.source_path.clone(),
            entry.output_path.clone(),
            entry.include_annotations.to_string(),
            entry.crop.clone(),
            entry.success.to_string(),
            entry.error.clone().unwrap_or_default(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::domain::annotation::{
        AnnotationKind, AnnotationPosition, AnnotationStyle, RectanglePosition,
    };
    use crate::review::storage::sqlite_repository::SqliteReviewRepository;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn exports_image_with_optional_annotation_burn() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("sample.png");
        let img = ImageBuffer::from_fn(64, 64, |_, _| Rgba([120u8, 120, 120, 255]));
        img.save(&source).unwrap();

        let repo = SqliteReviewRepository::open_memory().unwrap();
        let batch_id = repo.create_batch("shots", &[source.clone()]).unwrap();
        let image_id = repo.list_images(batch_id, &Default::default()).unwrap()[0].id;
        let ann = crate::review::domain::Annotation::new_draft(
            image_id,
            AnnotationKind::Rectangle,
            AnnotationPosition::Rectangle(RectanglePosition {
                x0: 0.1,
                y0: 0.1,
                x1: 0.4,
                y1: 0.4,
            }),
            AnnotationStyle::default(),
            "issue".into(),
        );
        repo.insert_annotation(&ann).unwrap();

        let out_dir = dir.path().join("exports");
        let request = BatchImageScreenshotRequest {
            items: vec![(image_id, source.clone())],
            output_dir: out_dir.clone(),
            include_annotations: true,
            format: ScreenshotFormat::Png,
            quality: 85,
            naming_template: "{index}_{filename}.{ext}".into(),
            write_csv_manifest: true,
            write_json_manifest: false,
            write_contact_sheet: false,
            crop: None,
        };
        let result = BatchImageScreenshotService::export(&repo, &request, None).unwrap();
        assert_eq!(result.succeeded, 1);
        assert!(result.output_files[0].exists());
        assert!(result.csv_manifest.as_ref().is_some_and(|p| p.exists()));
    }

    #[test]
    fn export_paths_does_not_need_repo() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("plain.png");
        let img = ImageBuffer::from_fn(32, 32, |_, _| Rgba([10u8, 20, 30, 255]));
        img.save(&source).unwrap();

        let out_dir = dir.path().join("out");
        let request = BatchImageScreenshotRequest::from_paths(vec![source], out_dir.clone());
        let result = BatchImageScreenshotService::export_paths(&request, None).unwrap();
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.manifest_entries[0].image_id, 0);
        assert!(!result.manifest_entries[0].include_annotations);
        assert_eq!(result.manifest_entries[0].crop, "full");
        assert!(result.output_files[0].exists());
        assert!(out_dir.join("image_screenshots.csv").exists());
    }

    #[test]
    fn apply_norm_crop_clamps_and_sizes() {
        let img = ImageBuffer::from_fn(100, 80, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let cropped = apply_norm_crop(
            &img,
            NormRect {
                x0: 0.1,
                y0: 0.2,
                x1: 0.5,
                y1: 0.7,
            },
        )
        .unwrap();
        assert_eq!(cropped.width(), 40);
        assert_eq!(cropped.height(), 40);

        // 越界夹紧
        let clamped = apply_norm_crop(
            &img,
            NormRect {
                x0: -0.5,
                y0: 0.0,
                x1: 2.0,
                y1: 1.0,
            },
        )
        .unwrap();
        assert_eq!(clamped.width(), 100);
        assert_eq!(clamped.height(), 80);
    }

    #[test]
    fn export_paths_applies_crop() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("big.png");
        let img = ImageBuffer::from_fn(100, 100, |_, _| Rgba([200u8, 100, 50, 255]));
        img.save(&source).unwrap();

        let out_dir = dir.path().join("cropped");
        let mut request = BatchImageScreenshotRequest::from_paths(vec![source], out_dir);
        request.format = ScreenshotFormat::Png;
        request.crop = Some(NormRect {
            x0: 0.25,
            y0: 0.25,
            x1: 0.75,
            y1: 0.75,
        });
        let result = BatchImageScreenshotService::export_paths(&request, None).unwrap();
        assert_eq!(result.succeeded, 1);
        let out = image::open(&result.output_files[0]).unwrap();
        assert_eq!(out.width(), 50);
        assert_eq!(out.height(), 50);
        assert!(result.manifest_entries[0].crop.starts_with("roi="));
    }

    #[test]
    fn tiny_roi_treated_as_full() {
        assert!(!is_meaningful_roi(&NormRect {
            x0: 0.5,
            y0: 0.5,
            x1: 0.501,
            y1: 0.501,
        }));
        assert_eq!(
            format_roi_label(Some(NormRect {
                x0: 0.5,
                y0: 0.5,
                x1: 0.501,
                y1: 0.502,
            })),
            "full"
        );
    }
}
