//! 按参考图匹配外观（优先 Vitrine 风格矩阵+曲线+LUT，失败回退亮度增益）。
//!
//! 速度与精度策略：
//! - 主路径：[`crate::processing::camera_match`] 在 200×150 网格上拟合
//!   ridge 3×3 + PAVA 曲线 + 13³ 残差 LUT，再施加到原图；
//! - 回退：分析分辨率 luma 统计 + 全局/分区增益（参考图缓存长边 ≤ [`REF_CACHE_MAX_EDGE`]）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use image::{DynamicImage, GenericImageView, RgbaImage};

use crate::core::error::{AppError, AppResult};
use crate::core::types::{BrightnessMatchMetric, BrightnessMatchMode, BrightnessMatchOptions};

const MIN_GAIN: f32 = 0.25;
const MAX_GAIN: f32 = 4.0;
const EPS: f32 = 1e-3;

/// 参考图缓存长边上限（保留足够细节供分析缩放）。
pub const REF_CACHE_MAX_EDGE: u32 = 2048;
/// 亮度统计 / 分区增益计算用分析图长边上限。
pub const ANALYSIS_MAX_EDGE: u32 = 1024;

/// 批任务内共享的亮度匹配会话。
///
/// - `global`：非 RAW 在「全局参考」模式下使用
/// - `paired`：同名参考缓存；**RAW 始终走配对**（贴近同名 JPG）
#[derive(Debug, Clone)]
pub struct BrightnessMatchCache {
    pub(crate) global: Option<Arc<DynamicImage>>,
    paired: Arc<Mutex<HashMap<PathBuf, Arc<DynamicImage>>>>,
}

impl BrightnessMatchCache {
    fn empty_paired() -> Arc<Mutex<HashMap<PathBuf, Arc<DynamicImage>>>> {
        Arc::new(Mutex::new(HashMap::new()))
    }

    pub fn try_from_options(options: &BrightnessMatchOptions) -> AppResult<Option<Self>> {
        if !options.is_active() {
            return Ok(None);
        }
        let global = if options.mode == BrightnessMatchMode::Global {
            let path = options.reference_path.as_ref().ok_or_else(|| {
                AppError::Config("brightness_match.reference_path missing".into())
            })?;
            Some(Arc::new(open_reference_cached(path)?))
        } else {
            None
        };
        Ok(Some(Self {
            global,
            paired: Self::empty_paired(),
        }))
    }

    /// 解析本张图应使用的参考图。
    ///
    /// RAW：始终同名 JPG/PNG/WebP（目标：转换后亮度贴近直出）；无配对则 `Ok(None)`。  
    /// 非 RAW：按 `options.mode` 使用全局参考或同名配对。
    pub fn resolve_reference(
        &self,
        source: &Path,
        options: &BrightnessMatchOptions,
    ) -> AppResult<Option<Arc<DynamicImage>>> {
        if crate::processing::backends::is_raw_camera_path(source) {
            return self.load_paired(source);
        }
        match options.mode {
            BrightnessMatchMode::Global => Ok(self.global.as_ref().map(Arc::clone)),
            BrightnessMatchMode::Paired => self.load_paired(source),
        }
    }

    fn load_paired(&self, source: &Path) -> AppResult<Option<Arc<DynamicImage>>> {
        let Some(path) = crate::io::reference_pick::find_paired_reference(source) else {
            return Ok(None);
        };
        {
            let guard = self
                .paired
                .lock()
                .map_err(|_| AppError::Other("brightness match paired cache poisoned".into()))?;
            if let Some(img) = guard.get(&path) {
                return Ok(Some(Arc::clone(img)));
            }
        }
        let img = Arc::new(open_reference_cached(&path)?);
        let mut guard = self
            .paired
            .lock()
            .map_err(|_| AppError::Other("brightness match paired cache poisoned".into()))?;
        Ok(Some(Arc::clone(
            guard.entry(path).or_insert_with(|| Arc::clone(&img)),
        )))
    }
}

fn open_reference_cached(path: &Path) -> AppResult<DynamicImage> {
    let image = image::open(path).map_err(|e| AppError::DecodeFailed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    Ok(downscale_max_edge(&image, REF_CACHE_MAX_EDGE))
}

/// 长边超过 `max_edge` 时等比缩小，否则原样克隆。
pub fn downscale_max_edge(image: &DynamicImage, max_edge: u32) -> DynamicImage {
    let w = image.width().max(1);
    let h = image.height().max(1);
    let long = w.max(h);
    if long <= max_edge {
        return image.clone();
    }
    let scale = max_edge as f32 / long as f32;
    let nw = ((w as f32) * scale).round().max(1.0) as u32;
    let nh = ((h as f32) * scale).round().max(1.0) as u32;
    image.resize(nw, nh, image::imageops::FilterType::Triangle)
}

/// 由源图尺寸推导分析分辨率（保持宽高比，长边 ≤ max_edge）。
pub fn analysis_size(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let w = width.max(1);
    let h = height.max(1);
    let long = w.max(h);
    if long <= max_edge {
        return (w, h);
    }
    let scale = max_edge as f32 / long as f32;
    (
        ((w as f32) * scale).round().max(1.0) as u32,
        ((h as f32) * scale).round().max(1.0) as u32,
    )
}

fn to_analysis_rgba(image: &DynamicImage, tw: u32, th: u32) -> RgbaImage {
    if image.width() == tw && image.height() == th {
        return image.to_rgba8();
    }
    image
        .resize_exact(tw, th, image::imageops::FilterType::Triangle)
        .to_rgba8()
}

#[inline]
pub fn luma(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
}

pub fn clamp_gain(k: f32) -> f32 {
    k.clamp(MIN_GAIN, MAX_GAIN)
}

/// 对整图采样 luma 并计算均值或百分位（公开 API，走全像素；批处理请用分析路径）。
pub fn image_luma_stat(
    image: &DynamicImage,
    metric: BrightnessMatchMetric,
    percentile: f32,
) -> f32 {
    let rgba = image.to_rgba8();
    region_luma_stat(&rgba, 0, 0, rgba.width(), rgba.height(), metric, percentile)
}

fn region_luma_stat(
    rgba: &RgbaImage,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    metric: BrightnessMatchMetric,
    percentile: f32,
) -> f32 {
    let w = rgba.width();
    let h = rgba.height();
    let x0 = x0.min(w);
    let y0 = y0.min(h);
    let x1 = x1.min(w).max(x0);
    let y1 = y1.min(h).max(y0);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }

    let area = ((x1 - x0) * (y1 - y0)) as usize;
    match metric {
        BrightnessMatchMetric::Mean => {
            let mut sum = 0.0f32;
            let mut n = 0usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = rgba.get_pixel(x, y).0;
                    sum += luma(p[0], p[1], p[2]);
                    n += 1;
                }
            }
            if n == 0 {
                0.0
            } else {
                sum / n as f32
            }
        }
        BrightnessMatchMetric::Percentile => {
            let mut values = Vec::with_capacity(area);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = rgba.get_pixel(x, y).0;
                    values.push(luma(p[0], p[1], p[2]));
                }
            }
            if values.is_empty() {
                return 0.0;
            }
            percentile_of(&mut values, percentile)
        }
    }
}

fn percentile_of(values: &mut [f32], percentile: f32) -> f32 {
    let p = percentile.clamp(0.0, 100.0) / 100.0;
    let idx = ((values.len() as f32 - 1.0) * p).round() as usize;
    let idx = idx.min(values.len() - 1);
    let (_, nth, _) = values.select_nth_unstable_by(idx, |a, b| {
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
    });
    *nth
}

pub fn gain_from_stats(ref_stat: f32, src_stat: f32) -> f32 {
    if src_stat < EPS {
        return 1.0;
    }
    clamp_gain(ref_stat / src_stat)
}

/// 将参考图缩放到与目标相同尺寸（仅用于统计，不写盘）。
pub fn resize_reference_to(reference: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    if reference.width() == width && reference.height() == height {
        return reference.clone();
    }
    reference.resize_exact(width, height, image::imageops::FilterType::Triangle)
}

pub fn apply_global_gain(image: &mut DynamicImage, gain: f32) {
    if (gain - 1.0).abs() < 1e-4 {
        return;
    }
    let mut rgba = image.to_rgba8();
    let buf = rgba.as_mut();
    for px in buf.chunks_exact_mut(4) {
        px[0] = scale_channel(px[0], gain);
        px[1] = scale_channel(px[1], gain);
        px[2] = scale_channel(px[2], gain);
    }
    *image = DynamicImage::ImageRgba8(rgba);
}

#[inline]
fn scale_channel(v: u8, gain: f32) -> u8 {
    (v as f32 * gain).round().clamp(0.0, 255.0) as u8
}

/// 在已对齐的分析分辨率 RGBA 上计算分区增益。
fn compute_grid_gains_rgba(
    reference: &RgbaImage,
    source: &RgbaImage,
    options: &BrightnessMatchOptions,
) -> Vec<f32> {
    let cols = options.grid_cols.max(1);
    let rows = options.grid_rows.max(1);
    let w = source.width();
    let h = source.height();
    debug_assert_eq!(reference.width(), w);
    debug_assert_eq!(reference.height(), h);
    let mut gains = Vec::with_capacity((cols * rows) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let x0 = col * w / cols;
            let x1 = (col + 1) * w / cols;
            let y0 = row * h / rows;
            let y1 = (row + 1) * h / rows;
            let ref_stat = region_luma_stat(
                reference,
                x0,
                y0,
                x1,
                y1,
                options.metric,
                options.percentile,
            );
            let src_stat =
                region_luma_stat(source, x0, y0, x1, y1, options.metric, options.percentile);
            gains.push(gain_from_stats(ref_stat, src_stat));
        }
    }
    gains
}

/// 计算分区增益（行主序 grid_rows * grid_cols）；公开 API，内部走分析分辨率。
pub fn compute_grid_gains(
    reference: &DynamicImage,
    source: &DynamicImage,
    options: &BrightnessMatchOptions,
) -> Vec<f32> {
    let (aw, ah) = analysis_size(source.width(), source.height(), ANALYSIS_MAX_EDGE);
    let ref_a = to_analysis_rgba(reference, aw, ah);
    let src_a = to_analysis_rgba(source, aw, ah);
    compute_grid_gains_rgba(&ref_a, &src_a, options)
}

/// 分区增益场：先把格点增益双线性铺到分析分辨率图，再最近邻映射到原图（亮度场低频，足够准且更快）。
pub fn apply_grid_gains(image: &mut DynamicImage, gains: &[f32], cols: u32, rows: u32) {
    let cols = cols.max(1);
    let rows = rows.max(1);
    if gains.len() != (cols * rows) as usize {
        return;
    }
    if cols == 1 && rows == 1 {
        apply_global_gain(image, gains[0]);
        return;
    }

    let mut rgba = image.to_rgba8();
    let w = rgba.width().max(1);
    let h = rgba.height().max(1);
    let (aw, ah) = analysis_size(w, h, ANALYSIS_MAX_EDGE);
    let gain_map = rasterize_gain_map(gains, cols, rows, aw, ah);

    let buf = rgba.as_mut();
    let stride = (w as usize) * 4;
    for y in 0..h {
        let gy = ((y as u64 * ah as u64) / h as u64).min(ah as u64 - 1) as u32;
        let row = &mut buf[y as usize * stride..y as usize * stride + stride];
        let map_row = gy * aw;
        for x in 0..w {
            let gx = ((x as u64 * aw as u64) / w as u64).min(aw as u64 - 1) as u32;
            let gain = gain_map[(map_row + gx) as usize];
            if (gain - 1.0).abs() < 1e-4 {
                continue;
            }
            let i = (x as usize) * 4;
            row[i] = scale_channel(row[i], gain);
            row[i + 1] = scale_channel(row[i + 1], gain);
            row[i + 2] = scale_channel(row[i + 2], gain);
        }
    }
    *image = DynamicImage::ImageRgba8(rgba);
}

fn rasterize_gain_map(gains: &[f32], cols: u32, rows: u32, aw: u32, ah: u32) -> Vec<f32> {
    let mut map = vec![1.0f32; (aw * ah) as usize];
    let inv_w = cols as f32 / aw as f32;
    let inv_h = rows as f32 / ah as f32;
    for y in 0..ah {
        let fy = (y as f32 + 0.5) * inv_h - 0.5;
        let y0 = fy.floor() as i32;
        let ty = fy - y0 as f32;
        for x in 0..aw {
            let fx = (x as f32 + 0.5) * inv_w - 0.5;
            let x0 = fx.floor() as i32;
            let tx = fx - x0 as f32;
            let g00 = sample_gain(gains, cols, rows, x0, y0);
            let g10 = sample_gain(gains, cols, rows, x0 + 1, y0);
            let g01 = sample_gain(gains, cols, rows, x0, y0 + 1);
            let g11 = sample_gain(gains, cols, rows, x0 + 1, y0 + 1);
            map[(y * aw + x) as usize] = g00 * (1.0 - tx) * (1.0 - ty)
                + g10 * tx * (1.0 - ty)
                + g01 * (1.0 - tx) * ty
                + g11 * tx * ty;
        }
    }
    map
}

fn sample_gain(gains: &[f32], cols: u32, rows: u32, x: i32, y: i32) -> f32 {
    let cx = x.clamp(0, cols as i32 - 1) as u32;
    let cy = y.clamp(0, rows as i32 - 1) as u32;
    gains[(cy * cols + cx) as usize]
}

/// 对源图应用亮度匹配（统计在分析分辨率，增益在原图施加）。
/// 对源图应用参考匹配。
///
/// 优先 Vitrine 风格相机匹配（矩阵 + 单调曲线 + 残差 LUT）；拟合失败时
/// fail-open 回退到亮度增益 / 分区增益。
pub fn apply_brightness_match(
    image: &mut DynamicImage,
    reference: &DynamicImage,
    options: &BrightnessMatchOptions,
) {
    if crate::processing::camera_match::try_apply_camera_match(image, reference) {
        return;
    }
    apply_brightness_match_gain_fallback(image, reference, options);
}

/// 简单亮度增益回退路径（相机匹配不可用时）。
pub fn apply_brightness_match_gain_fallback(
    image: &mut DynamicImage,
    reference: &DynamicImage,
    options: &BrightnessMatchOptions,
) {
    if options.regional && (options.grid_cols > 1 || options.grid_rows > 1) {
        let (aw, ah) = analysis_size(image.width(), image.height(), ANALYSIS_MAX_EDGE);
        let ref_a = to_analysis_rgba(reference, aw, ah);
        let src_a = to_analysis_rgba(image, aw, ah);
        let gains = compute_grid_gains_rgba(&ref_a, &src_a, options);
        apply_grid_gains(
            image,
            &gains,
            options.grid_cols.max(1),
            options.grid_rows.max(1),
        );
        return;
    }

    let ref_stat = luma_stat_analysis(reference, options.metric, options.percentile);
    let src_stat = luma_stat_analysis(image, options.metric, options.percentile);
    apply_global_gain(image, gain_from_stats(ref_stat, src_stat));
}

fn luma_stat_analysis(image: &DynamicImage, metric: BrightnessMatchMetric, percentile: f32) -> f32 {
    let small = downscale_max_edge(image, ANALYSIS_MAX_EDGE);
    let rgba = small.to_rgba8();
    region_luma_stat(&rgba, 0, 0, rgba.width(), rgba.height(), metric, percentile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use std::time::Instant;

    fn solid(w: u32, h: u32, v: u8) -> DynamicImage {
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(w, h, |_, _| Rgba([v, v, v, 255])))
    }

    fn opts(metric: BrightnessMatchMetric, regional: bool) -> BrightnessMatchOptions {
        BrightnessMatchOptions {
            enabled: true,
            mode: BrightnessMatchMode::Global,
            reference_path: Some("ref.jpg".into()),
            metric,
            percentile: 98.0,
            regional,
            grid_cols: 3,
            grid_rows: 3,
        }
    }

    #[test]
    fn mean_gain_brightens_dark_image() {
        let reference = solid(32, 32, 200);
        let mut source = solid(32, 32, 50);
        apply_brightness_match(
            &mut source,
            &reference,
            &opts(BrightnessMatchMetric::Mean, false),
        );
        let p = source.to_rgba8().get_pixel(0, 0).0;
        assert!(p[0] > 150, "expected brightened pixel, got {:?}", p);
    }

    #[test]
    fn percentile_stat_uses_high_tail() {
        let mut rgba = image::RgbaImage::from_pixel(100, 1, Rgba([10, 10, 10, 255]));
        for x in 90..100 {
            rgba.put_pixel(x, 0, Rgba([250, 250, 250, 255]));
        }
        let img = DynamicImage::ImageRgba8(rgba);
        let mean = image_luma_stat(&img, BrightnessMatchMetric::Mean, 98.0);
        let p98 = image_luma_stat(&img, BrightnessMatchMetric::Percentile, 98.0);
        assert!(p98 > mean);
        assert!(p98 > 200.0);
    }

    #[test]
    fn grid_gains_length_and_apply() {
        let reference = solid(30, 30, 180);
        let mut source = solid(30, 30, 60);
        let options = opts(BrightnessMatchMetric::Mean, true);
        let gains = compute_grid_gains(&reference, &source, &options);
        assert_eq!(gains.len(), 9);
        assert!(gains.iter().all(|g| *g > 1.0));
        apply_grid_gains(&mut source, &gains, 3, 3);
        let p = source.to_rgba8().get_pixel(15, 15).0;
        assert!(p[0] > 100);
    }

    #[test]
    fn gain_clamped_and_dark_src_skipped() {
        assert!((gain_from_stats(100.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((clamp_gain(100.0) - MAX_GAIN).abs() < 1e-6);
        assert!((clamp_gain(0.01) - MIN_GAIN).abs() < 1e-6);
    }

    #[test]
    fn paired_mode_is_active_without_path() {
        let options = BrightnessMatchOptions {
            enabled: true,
            mode: BrightnessMatchMode::Paired,
            ..Default::default()
        };
        assert!(options.is_active());
        assert!(!options.requires_global_reference());
        let cache = BrightnessMatchCache::try_from_options(&options).unwrap();
        assert!(cache.is_some());
        assert!(cache.unwrap().global.is_none());
    }

    #[test]
    fn raw_resolves_via_paired_even_in_global_mode() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("imgforge_bm_raw_{nanos}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // 1×1 JPG as companion
        let jpg = dir.join("shot.jpg");
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([200, 200, 200, 255]),
        ))
        .save(&jpg)
        .unwrap();
        let raw = dir.join("shot.CR2");
        fs::write(&raw, b"not-a-real-raw").unwrap();
        let global = dir.join("global.jpg");
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([10, 10, 10, 255]),
        ))
        .save(&global)
        .unwrap();

        let options = BrightnessMatchOptions {
            enabled: true,
            mode: BrightnessMatchMode::Global,
            reference_path: Some(global.clone()),
            ..Default::default()
        };
        let cache = BrightnessMatchCache::try_from_options(&options)
            .unwrap()
            .unwrap();
        let resolved = cache.resolve_reference(&raw, &options).unwrap().unwrap();
        // 应使用同名 JPG（亮），而非全局参考（暗）
        let p = resolved.to_rgba8().get_pixel(0, 0).0;
        assert!(p[0] > 100, "expected paired jpg luma, got {:?}", p);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_brightness_keeps_pixels() {
        let reference = solid(64, 64, 120);
        let mut source = solid(64, 64, 120);
        apply_brightness_match(
            &mut source,
            &reference,
            &opts(BrightnessMatchMetric::Mean, false),
        );
        let p = source.to_rgba8().get_pixel(0, 0).0;
        assert_eq!(p[0], 120);
    }

    #[test]
    fn analysis_match_tracks_full_res_gain_ratio() {
        // 参考亮、源暗：分析路径应给出接近 2× 的增益（在 clamp 内）。
        let reference = solid(64, 48, 200);
        let mut source = solid(4000, 3000, 100);
        apply_brightness_match(
            &mut source,
            &reference,
            &opts(BrightnessMatchMetric::Mean, false),
        );
        let p = source.to_rgba8().get_pixel(0, 0).0;
        assert!((p[0] as i32 - 200).abs() <= 2, "got {}", p[0]);
    }

    #[test]
    fn analysis_size_caps_long_edge() {
        let (w, h) = analysis_size(6000, 4000, ANALYSIS_MAX_EDGE);
        assert!(w.max(h) <= ANALYSIS_MAX_EDGE);
        assert!((w as f32 / h as f32 - 1.5).abs() < 0.02);
    }

    #[test]
    fn large_image_match_is_fast() {
        let reference = solid(1024, 768, 180);
        let mut source = solid(4000, 3000, 60);
        let start = Instant::now();
        apply_brightness_match(
            &mut source,
            &reference,
            &opts(BrightnessMatchMetric::Percentile, true),
        );
        let elapsed = start.elapsed();
        // debug 构建偏慢；release 通常远低于此。亮度场走分析分辨率，应避免全像素排序。
        let limit_ms = if cfg!(debug_assertions) { 8_000 } else { 1_500 };
        assert!(
            elapsed.as_millis() < limit_ms,
            "brightness match too slow: {elapsed:?} (limit {limit_ms}ms)"
        );
        let p = source.to_rgba8().get_pixel(10, 10).0;
        assert!(p[0] > 100, "expected brightened, got {:?}", p);
    }
}
