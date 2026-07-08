//! 图片评审分析：直方图、亮度与裁切比例。

use std::path::Path;

use image::GenericImageView;
use serde::{Deserialize, Serialize};

use crate::review::error::{ReviewError, ReviewResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageAnalysis {
    pub width: u32,
    pub height: u32,
    pub luminance_histogram: Vec<u32>,
    pub red_histogram: Vec<u32>,
    pub green_histogram: Vec<u32>,
    pub blue_histogram: Vec<u32>,
    pub average_luminance: f32,
    pub shadow_clip_ratio: f32,
    pub highlight_clip_ratio: f32,
}

pub struct ImageAnalysisService;

impl ImageAnalysisService {
    pub fn analyze(path: &Path) -> ReviewResult<ImageAnalysis> {
        let img = image::open(path).map_err(|source| ReviewError::ImageDecode {
            path: path.to_path_buf(),
            source,
        })?;
        let (src_w, src_h) = img.dimensions();
        let img = img.thumbnail(1024, 1024).to_rgba8();
        let mut lum = vec![0u32; 256];
        let mut red = vec![0u32; 256];
        let mut green = vec![0u32; 256];
        let mut blue = vec![0u32; 256];
        let mut lum_sum = 0f64;
        let mut shadow = 0u32;
        let mut highlight = 0u32;
        let mut total = 0u32;

        for p in img.pixels() {
            let [r, g, b, a] = p.0;
            if a == 0 {
                continue;
            }
            red[r as usize] += 1;
            green[g as usize] += 1;
            blue[b as usize] += 1;
            let y = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32).round() as u8;
            lum[y as usize] += 1;
            lum_sum += y as f64;
            if y <= 3 {
                shadow += 1;
            }
            if y >= 252 {
                highlight += 1;
            }
            total += 1;
        }

        let denom = total.max(1) as f32;
        Ok(ImageAnalysis {
            width: src_w,
            height: src_h,
            luminance_histogram: lum,
            red_histogram: red,
            green_histogram: green,
            blue_histogram: blue,
            average_luminance: (lum_sum / denom as f64) as f32,
            shadow_clip_ratio: shadow as f32 / denom,
            highlight_clip_ratio: highlight as f32 / denom,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn analyzes_black_and_white_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bw.png");
        let img = ImageBuffer::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([0u8, 0, 0, 255])
            } else {
                Rgba([255u8, 255, 255, 255])
            }
        });
        img.save(&path).unwrap();
        let analysis = ImageAnalysisService::analyze(&path).unwrap();
        assert!(analysis.luminance_histogram[0] > 0);
        assert!(analysis.luminance_histogram[255] > 0);
        assert!(analysis.shadow_clip_ratio > 0.0);
        assert!(analysis.highlight_clip_ratio > 0.0);
    }
}
