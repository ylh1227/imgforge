//! 直方图分析（CPU）与绘制缓冲。

use image::RgbaImage;

use super::color::luma709_f32;

/// Y 软分箱定点权重（1.0 == WEIGHT_SCALE）。
const Y_WEIGHT_SCALE: u64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistogramBins {
    /// 软分箱累加（定点）；显示前按权重归一化。
    pub y: [u64; 256],
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
}

impl Default for HistogramBins {
    fn default() -> Self {
        Self {
            y: [0; 256],
            r: [0; 256],
            g: [0; 256],
            b: [0; 256],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum HistogramMode {
    #[default]
    Parade = 0,
    Overlay = 1,
    Stack = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum HistogramScale {
    #[default]
    Linear = 0,
    Log = 1,
}

/// 将浮点亮度按线性插值分到相邻两档，减少 round-to-u8 损失。
#[inline]
pub fn soft_bin_y(bins: &mut [u64; 256], y: f32) {
    let y = y.clamp(0.0, 255.0);
    if y >= 255.0 {
        bins[255] = bins[255].saturating_add(Y_WEIGHT_SCALE);
        return;
    }
    let i0 = y.floor() as usize;
    let f = (y - i0 as f32) as f64;
    let w1 = (f * Y_WEIGHT_SCALE as f64).round() as u64;
    let w0 = Y_WEIGHT_SCALE.saturating_sub(w1);
    bins[i0] = bins[i0].saturating_add(w0);
    bins[i0 + 1] = bins[i0 + 1].saturating_add(w1);
}

pub fn analyze(img: &RgbaImage) -> HistogramBins {
    let mut bins = HistogramBins::default();
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        if a == 0 {
            continue;
        }
        bins.r[r as usize] += 1;
        bins.g[g as usize] += 1;
        bins.b[b as usize] += 1;
        soft_bin_y(&mut bins.y, luma709_f32(r, g, b));
    }
    bins
}

/// 将 bins 归一化为 0..255 高度图（4 行：Y,R,G,B），供 GPU 纹理上传。
pub fn bins_to_height_map(bins: &HistogramBins, scale: HistogramScale) -> [u8; 256 * 4] {
    let mut out = [0u8; 256 * 4];
    let y_max = bins.y.iter().copied().max().unwrap_or(1).max(1) as f64;
    for (i, &count) in bins.y.iter().enumerate() {
        let t = height_t(count as f64, y_max, scale);
        out[i] = (t * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    for (row, channel) in [&bins.r, &bins.g, &bins.b].iter().enumerate() {
        let max = channel.iter().copied().max().unwrap_or(1).max(1) as f64;
        for (i, &count) in channel.iter().enumerate() {
            let t = height_t(count as f64, max, scale);
            out[(row + 1) * 256 + i] = (t * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn height_t(count: f64, max: f64, scale: HistogramScale) -> f64 {
    if count <= 0.0 {
        return 0.0;
    }
    match scale {
        HistogramScale::Linear => count / max,
        HistogramScale::Log => (1.0 + count).ln() / (1.0 + max).ln(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn black_and_white_peaks() {
        let img = ImageBuffer::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        let bins = analyze(&img);
        assert!(bins.y[0] > 0);
        assert!(bins.y[255] > 0);
        assert!(bins.r[0] > 0);
        assert!(bins.r[255] > 0);
    }

    #[test]
    fn pure_red_peaks_red_channel() {
        let img = ImageBuffer::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
        let bins = analyze(&img);
        assert_eq!(bins.r[255], 16);
        assert_eq!(bins.g[0], 16);
        assert_eq!(bins.b[0], 16);
    }

    #[test]
    fn soft_bin_splits_fractional_luma() {
        let mut bins = [0u64; 256];
        // 正好落在 10.25 → 75% @10, 25% @11
        soft_bin_y(&mut bins, 10.25);
        assert_eq!(bins[10], Y_WEIGHT_SCALE * 3 / 4);
        assert_eq!(bins[11], Y_WEIGHT_SCALE / 4);
    }
}
