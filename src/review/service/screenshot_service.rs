//! 图片评审批量截图导出。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::types::{ImageFormat, Quality};
use crate::processing::backends::native_backend::encode_dynamic_image;
use crate::review::domain::burn_annotations_onto;
use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::SqliteReviewRepository;
use crate::ui::progress::ProgressReporter;
use crate::video_review::service::contact_sheet::ContactSheetService;
use crate::video_review::service::screenshot_service::{
    render_filename, unique_output_path, ScreenshotFormat,
};

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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageScreenshotManifestEntry {
    pub index: usize,
    pub image_id: i64,
    pub source_path: String,
    pub output_path: String,
    pub include_annotations: bool,
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
        if request.items.is_empty() {
            return Err(ReviewError::Message("没有可导出的图片".into()));
        }
        fs::create_dir_all(&request.output_dir)?;

        if let Some(p) = progress {
            p.set_total(request.items.len());
        }

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
            let entry = export_one(repo, request, *image_id, source, index + 1)?;
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
}

fn export_one(
    repo: &SqliteReviewRepository,
    request: &BatchImageScreenshotRequest,
    image_id: i64,
    source: &Path,
    index: usize,
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
    repo: &SqliteReviewRepository,
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
        let annotations = repo.list_annotations(image_id)?;
        burn_annotations_onto(&mut rgba, &annotations);
    }

    let target_format = match request.format {
        ScreenshotFormat::Jpeg => ImageFormat::Jpeg,
        ScreenshotFormat::Png => ImageFormat::Png,
    };
    let encoded = encode_dynamic_image(
        &image::DynamicImage::ImageRgba8(rgba),
        target_format,
        Quality::new(request.quality).unwrap_or(Quality::DEFAULT),
    )
    .map_err(|e| ReviewError::Message(e.to_string()))?;
    fs::write(dest, encoded)?;
    Ok(())
}

fn write_csv_manifest(path: &Path, entries: &[ImageScreenshotManifestEntry]) -> ReviewResult<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    for entry in entries {
        wtr.write_record([
            entry.index.to_string(),
            entry.image_id.to_string(),
            entry.source_path.clone(),
            entry.output_path.clone(),
            entry.include_annotations.to_string(),
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
        };
        let result = BatchImageScreenshotService::export(&repo, &request, None).unwrap();
        assert_eq!(result.succeeded, 1);
        assert!(result.output_files[0].exists());
        assert!(result.csv_manifest.as_ref().is_some_and(|p| p.exists()));
    }
}
