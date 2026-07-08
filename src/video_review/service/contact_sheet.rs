//! 多视频宫格 contact sheet 拼接导出。

use std::path::{Path, PathBuf};

use ab_glyph::PxScale;
use image::{imageops::FilterType, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect;

use crate::video_review::domain::VideoItem;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::ffmpeg_backend::ms_to_timestamp;

const GAP: u32 = 8;
const HEADER_H: u32 = 36;
const FOOTER_H: u32 = 22;
const DEFAULT_CELL_FRAME_W: u32 = 480;

/// 单页索引图最多单元格数。
pub const CONTACT_SHEET_PAGE_SIZE: usize = 36;

#[derive(Debug, Clone)]
pub struct ContactSheetRequest {
    pub videos: Vec<VideoItem>,
    pub time_ms: u64,
    pub dest: PathBuf,
    pub cell_frame_width: u32,
}

impl ContactSheetRequest {
    pub fn new(videos: Vec<VideoItem>, time_ms: u64, dest: PathBuf) -> Self {
        Self {
            videos,
            time_ms,
            dest,
            cell_frame_width: DEFAULT_CELL_FRAME_W,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactSheetResult {
    pub dest: PathBuf,
    pub width: u32,
    pub height: u32,
    pub rows: usize,
    pub cols: usize,
    pub video_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridLayout {
    pub rows: usize,
    pub cols: usize,
    pub cell_w: u32,
    pub cell_h: u32,
    pub sheet_w: u32,
    pub sheet_h: u32,
}

/// 计算宫格行列数（与 `multi_compare.rs` 规则一致）。
pub fn grid_dimensions(count: usize) -> (usize, usize) {
    if count == 0 {
        return (0, 0);
    }
    let cols = match count {
        1 => 1,
        2 => 2,
        3 | 4 => 2,
        _ => 3,
    };
    let rows = count.div_ceil(cols);
    (rows, cols)
}

pub fn compute_layout(count: usize, frame_w: u32, frame_h: u32) -> GridLayout {
    let (rows, cols) = grid_dimensions(count);
    if rows == 0 {
        return GridLayout {
            rows: 0,
            cols: 0,
            cell_w: 0,
            cell_h: 0,
            sheet_w: 0,
            sheet_h: 0,
        };
    }
    let cell_w = frame_w;
    let cell_h = HEADER_H + frame_h + FOOTER_H;
    let sheet_w = cols as u32 * cell_w + (cols as u32 + 1) * GAP;
    let sheet_h = rows as u32 * cell_h + (rows as u32 + 1) * GAP;
    GridLayout {
        rows,
        cols,
        cell_w,
        cell_h,
        sheet_w,
        sheet_h,
    }
}

pub trait FrameProvider {
    fn ensure_frame(
        &self,
        video: &VideoItem,
        global_time_ms: u64,
        width: u32,
    ) -> VideoReviewResult<PathBuf>;
}

pub struct ContactSheetService;

impl ContactSheetService {
    pub fn export<P: FrameProvider>(
        provider: &P,
        req: &ContactSheetRequest,
    ) -> VideoReviewResult<ContactSheetResult> {
        if req.videos.len() < 2 {
            return Err(VideoReviewError::Message(
                "宫格导出至少需要 2 个视频".into(),
            ));
        }
        if req.videos.len() > 6 {
            return Err(VideoReviewError::Message(
                "宫格导出最多支持 6 个视频".into(),
            ));
        }

        let frame_w = req.cell_frame_width.max(160);
        let frame_h = (frame_w as f32 * 9.0 / 16.0).round() as u32;
        let layout = compute_layout(req.videos.len(), frame_w, frame_h);

        let mut sheet =
            RgbaImage::from_pixel(layout.sheet_w, layout.sheet_h, Rgba([28, 28, 30, 255]));

        for (idx, video) in req.videos.iter().enumerate() {
            let row = idx / layout.cols;
            let col = idx % layout.cols;
            let x = GAP + col as u32 * (layout.cell_w + GAP);
            let y = GAP + row as u32 * (layout.cell_h + GAP);

            let effective = video.effective_time_ms(req.time_ms).min(video.duration_ms);
            let frame_path = provider.ensure_frame(video, req.time_ms, frame_w)?;
            let cell = render_cell(video, &frame_path, effective, frame_w, frame_h)?;
            image::imageops::overlay(&mut sheet, &cell, x as i64, y as i64);
        }

        if let Some(parent) = req.dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        sheet
            .save_with_format(&req.dest, image::ImageFormat::Png)
            .map_err(|e| VideoReviewError::Message(format!("保存 PNG 失败: {e}")))?;

        Ok(ContactSheetResult {
            dest: req.dest.clone(),
            width: layout.sheet_w,
            height: layout.sheet_h,
            rows: layout.rows,
            cols: layout.cols,
            video_count: req.videos.len(),
        })
    }

    /// 将已导出的截图文件拼成索引图（单页，最多 [`CONTACT_SHEET_PAGE_SIZE`] 张）。
    pub fn export_image_index(
        items: &[(PathBuf, String)],
        dest: PathBuf,
        cell_frame_width: u32,
    ) -> VideoReviewResult<ContactSheetResult> {
        let pages = Self::export_image_index_pages(
            items,
            dest.parent().unwrap_or(Path::new(".")),
            dest.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("screenshots_index"),
            cell_frame_width,
        )?;
        pages
            .into_iter()
            .next()
            .ok_or_else(|| VideoReviewError::Message("没有可拼图的截图".into()))
    }

    /// 按页导出索引图；超过 [`CONTACT_SHEET_PAGE_SIZE`] 张时自动分页。
    pub fn export_image_index_pages(
        items: &[(PathBuf, String)],
        output_dir: &Path,
        base_name: &str,
        cell_frame_width: u32,
    ) -> VideoReviewResult<Vec<ContactSheetResult>> {
        if items.is_empty() {
            return Err(VideoReviewError::Message("没有可拼图的截图".into()));
        }
        std::fs::create_dir_all(output_dir)?;
        let total_pages = items.len().div_ceil(CONTACT_SHEET_PAGE_SIZE);
        let mut results = Vec::with_capacity(total_pages);
        for (page_idx, chunk) in items.chunks(CONTACT_SHEET_PAGE_SIZE).enumerate() {
            let dest = index_page_path(output_dir, base_name, page_idx, total_pages);
            results.push(export_image_index_page(chunk, dest, cell_frame_width)?);
        }
        Ok(results)
    }
}

fn index_page_path(output_dir: &Path, base_name: &str, page_index: usize, total_pages: usize) -> PathBuf {
    if total_pages <= 1 {
        output_dir.join(format!("{base_name}.png"))
    } else {
        output_dir.join(format!("{base_name}_{:03}.png", page_index + 1))
    }
}

fn export_image_index_page(
    items: &[(PathBuf, String)],
    dest: PathBuf,
    cell_frame_width: u32,
) -> VideoReviewResult<ContactSheetResult> {
    let count = items.len();
    let frame_w = cell_frame_width.max(160);
    let frame_h = (frame_w as f32 * 9.0 / 16.0).round() as u32;
    let layout = compute_layout(count, frame_w, frame_h);
    let mut sheet =
        RgbaImage::from_pixel(layout.sheet_w, layout.sheet_h, Rgba([28, 28, 30, 255]));

    for (idx, (path, label)) in items.iter().enumerate() {
        let row = idx / layout.cols;
        let col = idx % layout.cols;
        let x = GAP + col as u32 * (layout.cell_w + GAP);
        let y = GAP + row as u32 * (layout.cell_h + GAP);
        let cell = render_index_cell(path, label, frame_w, frame_h)?;
        image::imageops::overlay(&mut sheet, &cell, x as i64, y as i64);
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    sheet
        .save_with_format(&dest, image::ImageFormat::Png)
        .map_err(|e| VideoReviewError::Message(format!("保存索引图失败: {e}")))?;

    Ok(ContactSheetResult {
        dest: dest.clone(),
        width: layout.sheet_w,
        height: layout.sheet_h,
        rows: layout.rows,
        cols: layout.cols,
        video_count: count,
    })
}

fn render_index_cell(
    image_path: &Path,
    label: &str,
    frame_w: u32,
    frame_h: u32,
) -> VideoReviewResult<RgbaImage> {
    let cell_h = HEADER_H + frame_h + FOOTER_H;
    let mut cell = RgbaImage::from_pixel(frame_w, cell_h, Rgba([40, 40, 44, 255]));
    draw_label(
        &mut cell,
        6,
        10,
        &truncate_label(label, 36),
        Rgba([230, 230, 235, 255]),
    );
    if let Ok(img) = image::open(image_path) {
        let thumb = img.resize_to_fill(frame_w, frame_h, FilterType::Triangle);
        image::imageops::overlay(&mut cell, &thumb.to_rgba8(), 0, HEADER_H as i64);
    }
    let name = image_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    draw_label(
        &mut cell,
        6,
        (HEADER_H + frame_h + 6) as i32,
        &truncate_label(&name, 28),
        Rgba([180, 180, 185, 255]),
    );
    Ok(cell)
}

fn render_cell(
    video: &VideoItem,
    frame_path: &Path,
    effective_ms: u64,
    frame_w: u32,
    frame_h: u32,
) -> VideoReviewResult<RgbaImage> {
    let cell_h = HEADER_H + frame_h + FOOTER_H;
    let mut cell = RgbaImage::from_pixel(frame_w, cell_h, Rgba([40, 40, 44, 255]));

    let status = video.status.color_rgba();
    draw_filled_rect_mut(
        &mut cell,
        Rect::at(0, 0).of_size(frame_w, 4),
        Rgba([status[0], status[1], status[2], 255]),
    );

    let name = video
        .file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("#{}", video.id));
    let header = truncate_label(&name, 28);
    draw_label(&mut cell, 6, 10, &header, Rgba([230, 230, 235, 255]));

    if let Ok(img) = image::open(frame_path) {
        let thumb = img.resize_to_fill(frame_w, frame_h, FilterType::Triangle);
        let rgba = thumb.to_rgba8();
        image::imageops::overlay(&mut cell, &rgba, 0, HEADER_H as i64);
    }

    let time_label = format!(
        "{} / {}",
        ms_to_timestamp(effective_ms),
        ms_to_timestamp(video.duration_ms)
    );
    draw_label(
        &mut cell,
        6,
        (HEADER_H + frame_h + 6) as i32,
        &time_label,
        Rgba([180, 180, 185, 255]),
    );

    if video.offset_ms != 0 {
        let offset = format!("offset {}ms", video.offset_ms);
        let ox = frame_w.saturating_sub(offset.len() as u32 * 7 + 8);
        draw_label(
            &mut cell,
            ox as i32,
            (HEADER_H + frame_h + 6) as i32,
            &offset,
            Rgba([140, 140, 145, 255]),
        );
    }

    Ok(cell)
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    format!(
        "{}…",
        chars[..max_chars.saturating_sub(1)]
            .iter()
            .collect::<String>()
    )
}

fn draw_label(img: &mut RgbaImage, x: i32, y: i32, text: &str, color: Rgba<u8>) {
    if let Some(font) = load_font() {
        draw_text_mut(img, color, x, y, PxScale::from(14.0), &font, text);
    }
}

fn load_font() -> Option<ab_glyph::FontArc> {
    for path in font_candidates() {
        if let Ok(data) = std::fs::read(&path) {
            if let Ok(font) = ab_glyph::FontArc::try_from_vec(data) {
                return Some(font);
            }
        }
    }
    None
}

pub(crate) fn font_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return vec![
            PathBuf::from("/System/Library/Fonts/Supplemental/Arial.ttf"),
            PathBuf::from("/Library/Fonts/Arial.ttf"),
            PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
        ];
    }
    #[cfg(target_os = "windows")]
    {
        return vec![PathBuf::from("C:\\Windows\\Fonts\\arial.ttf")];
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        return vec![
            PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
            PathBuf::from("/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"),
        ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn grid_dimensions_match_compare_rules() {
        assert_eq!(grid_dimensions(2), (1, 2));
        assert_eq!(grid_dimensions(3), (2, 2));
        assert_eq!(grid_dimensions(4), (2, 2));
        assert_eq!(grid_dimensions(5), (2, 3));
        assert_eq!(grid_dimensions(6), (2, 3));
    }

    #[test]
    fn compute_layout_sizes() {
        let layout = compute_layout(4, 480, 270);
        assert_eq!(layout.cols, 2);
        assert_eq!(layout.rows, 2);
        assert!(layout.sheet_w > 960);
        assert!(layout.sheet_h > 540);
    }

    struct TestProvider;

    impl FrameProvider for TestProvider {
        fn ensure_frame(
            &self,
            _video: &VideoItem,
            _global_time_ms: u64,
            _width: u32,
        ) -> VideoReviewResult<PathBuf> {
            let dir = tempdir().unwrap();
            let path = dir.path().join("frame.png");
            let img = RgbaImage::from_pixel(320, 180, Rgba([80, 120, 200, 255]));
            img.save(&path).unwrap();
            // leak temp dir for test duration - use keep()
            std::mem::forget(dir);
            Ok(path)
        }
    }

    fn sample_video(id: i64, name: &str) -> VideoItem {
        use crate::review::domain::image_item::ReviewStatus;
        use chrono::Utc;
        VideoItem {
            id,
            batch_id: 1,
            file_path: PathBuf::from(name),
            status: ReviewStatus::Pending,
            remark: None,
            thumbnail_path: None,
            duration_ms: 60_000,
            fps: 24.0,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            audio_codec: None,
            bitrate_kbps: None,
            device_model: None,
            offset_ms: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn export_contact_sheet_png() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("grid.png");
        let req = ContactSheetRequest::new(
            vec![sample_video(1, "/tmp/a.mp4"), sample_video(2, "/tmp/b.mp4")],
            1000,
            dest.clone(),
        );
        let result = ContactSheetService::export(&TestProvider, &req).unwrap();
        assert!(dest.exists());
        assert_eq!(result.video_count, 2);
        assert_eq!(result.cols, 2);
        let meta = fs::metadata(&dest).unwrap();
        assert!(meta.len() > 100);
    }

    #[test]
    fn export_image_index_sheet() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        RgbaImage::from_pixel(32, 18, Rgba([200, 40, 40, 255]))
            .save(&a)
            .unwrap();
        RgbaImage::from_pixel(32, 18, Rgba([40, 40, 200, 255]))
            .save(&b)
            .unwrap();
        let dest = dir.path().join("index.png");
        let items = vec![(a, "00:01.000".into()), (b, "00:02.000".into())];
        let result = ContactSheetService::export_image_index(&items, dest.clone(), 160).unwrap();
        assert!(dest.exists());
        assert_eq!(result.video_count, 2);
    }

    #[test]
    fn export_image_index_single_page_uses_base_name() {
        let dir = tempdir().unwrap();
        let mut items = Vec::new();
        for i in 0..12 {
            let path = dir.path().join(format!("shot_{i}.png"));
            RgbaImage::from_pixel(32, 18, Rgba([100, 100, 100, 255]))
                .save(&path)
                .unwrap();
            items.push((path, format!("label_{i}")));
        }
        let pages =
            ContactSheetService::export_image_index_pages(&items, dir.path(), "screenshots_index", 160)
                .unwrap();
        assert_eq!(pages.len(), 1);
        assert!(dir.path().join("screenshots_index.png").exists());
    }

    #[test]
    fn export_image_index_paginates_beyond_36() {
        let dir = tempdir().unwrap();
        let mut items = Vec::new();
        for i in 0..37 {
            let path = dir.path().join(format!("shot_{i}.png"));
            RgbaImage::from_pixel(32, 18, Rgba([100, 100, 100, 255]))
                .save(&path)
                .unwrap();
            items.push((path, format!("label_{i}")));
        }
        let pages =
            ContactSheetService::export_image_index_pages(&items, dir.path(), "screenshots_index", 160)
                .unwrap();
        assert_eq!(pages.len(), 2);
        assert!(dir.path().join("screenshots_index_001.png").exists());
        assert!(dir.path().join("screenshots_index_002.png").exists());
        assert_eq!(pages[0].video_count, 36);
        assert_eq!(pages[1].video_count, 1);
    }
}
