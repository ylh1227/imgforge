//! 波形监视器分析（按列累加）。

use image::RgbaImage;

use super::color::luma709;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum WaveformMode {
    #[default]
    Luma = 0,
    RgbParade = 1,
}

/// `width × 256` 密度；值越大表示该列该电平出现越多。
#[derive(Debug, Clone)]
pub struct WaveformData {
    pub width: u32,
    /// 交错存储：luma 为 `width*256`；RGB parade 为 `width*3*256`（R|G|B 并排）。
    pub bins: Vec<u32>,
    #[allow(dead_code)]
    pub mode: WaveformMode,
}

pub fn analyze(img: &RgbaImage, mode: WaveformMode) -> WaveformData {
    let w = img.width().max(1);
    let parade_w = match mode {
        WaveformMode::Luma => w,
        WaveformMode::RgbParade => w * 3,
    };
    let mut bins = vec![0u32; (parade_w * 256) as usize];

    for (x, _y, p) in img.enumerate_pixels() {
        let [r, g, b, a] = p.0;
        if a == 0 {
            continue;
        }
        match mode {
            WaveformMode::Luma => {
                let level = luma709(r, g, b) as u32;
                // y=0 在顶部表示高电平（示波器习惯：上白下黑）。
                let row = 255 - level;
                bins[(row * w + x) as usize] += 1;
            }
            WaveformMode::RgbParade => {
                for (ch, v) in [r, g, b].into_iter().enumerate() {
                    let col = x + ch as u32 * w;
                    let row = 255 - v as u32;
                    bins[(row * parade_w + col) as usize] += 1;
                }
            }
        }
    }

    WaveformData {
        width: parade_w,
        bins,
        mode,
    }
}

/// 归一化为 R 通道高度/强度图（单通道 u8），大小 `width × 256`。
pub fn to_intensity_map(data: &WaveformData) -> Vec<u8> {
    let max = data.bins.iter().copied().max().unwrap_or(1).max(1) as f32;
    data.bins
        .iter()
        .map(|&c| {
            if c == 0 {
                0
            } else {
                // 轻微压缩高亮，避免单点过曝。
                let t = (c as f32 / max).sqrt();
                (t * 255.0).round().clamp(0.0, 255.0) as u8
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn white_frame_peaks_top_row() {
        let img = ImageBuffer::from_pixel(8, 4, Rgba([255, 255, 255, 255]));
        let data = analyze(&img, WaveformMode::Luma);
        // row 0 = level 255
        let top: u32 = data.bins[..8].iter().sum();
        assert!(top > 0);
    }

    #[test]
    fn black_frame_peaks_bottom_row() {
        let img = ImageBuffer::from_pixel(8, 4, Rgba([0, 0, 0, 255]));
        let data = analyze(&img, WaveformMode::Luma);
        let bottom: u32 = data.bins[255 * 8..].iter().sum();
        assert!(bottom > 0);
    }
}
