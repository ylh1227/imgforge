//! 矢量示波器密度分析。

use image::RgbaImage;

use super::color::{cb_cr_to_scope_xy, rgb_to_cb_cr};

pub const VECTOR_SIZE: u32 = 256;

#[derive(Debug, Clone)]
pub struct VectorscopeData {
    #[allow(dead_code)]
    pub size: u32,
    pub density: Vec<u32>,
}

pub fn analyze(img: &RgbaImage) -> VectorscopeData {
    let size = VECTOR_SIZE;
    let mut density = vec![0u32; (size * size) as usize];
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        if a == 0 {
            continue;
        }
        let (cb, cr) = rgb_to_cb_cr(r, g, b);
        let (x, y) = cb_cr_to_scope_xy(cb, cr, size);
        density[(y * size + x) as usize] += 1;
    }
    VectorscopeData { size, density }
}

pub fn to_intensity_map(data: &VectorscopeData) -> Vec<u8> {
    let max = data.density.iter().copied().max().unwrap_or(1).max(1) as f32;
    data.density
        .iter()
        .map(|&c| {
            if c == 0 {
                0
            } else {
                let t = (c as f32 / max).sqrt();
                (t * 255.0).round().clamp(0.0, 255.0) as u8
            }
        })
        .collect()
}

/// 峰值坐标（用于测试象限）。
#[cfg(test)]
pub fn peak_xy(data: &VectorscopeData) -> Option<(u32, u32)> {
    let (idx, _) = data
        .density
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| *v)
        .filter(|(_, v)| **v > 0)?;
    let x = (idx as u32) % data.size;
    let y = (idx as u32) / data.size;
    Some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn pure_red_in_expected_quadrant() {
        let img = ImageBuffer::from_pixel(8, 8, Rgba([255, 0, 0, 255]));
        let data = analyze(&img);
        let (x, y) = peak_xy(&data).unwrap();
        // Cr 向上 → y 偏小；Cb 负 → x 偏小。
        assert!(x < 128, "red x={x}");
        assert!(y < 128, "red y={y}");
    }

    #[test]
    fn pure_blue_in_expected_quadrant() {
        let img = ImageBuffer::from_pixel(8, 8, Rgba([0, 0, 255, 255]));
        let data = analyze(&img);
        let (x, y) = peak_xy(&data).unwrap();
        assert!(x > 128, "blue x={x}");
    }
}
