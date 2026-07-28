//! BT.709 颜色转换与示波器刻度常量。

/// 视频合法电平参考（8-bit full/studio 常用刻度）。
#[allow(dead_code)]
pub const LEVEL_BLACK: u8 = 0;
#[allow(dead_code)]
pub const LEVEL_FOOTROOM: u8 = 16;
#[allow(dead_code)]
pub const LEVEL_PEAK: u8 = 235;
#[allow(dead_code)]
pub const LEVEL_WHITE: u8 = 255;

/// Rec.709 亮度（与图片评审直方图权重一致）。
#[inline]
pub fn luma709(r: u8, g: u8, b: u8) -> u8 {
    luma709_f32(r, g, b).round().clamp(0.0, 255.0) as u8
}

/// 未量化的 Rec.709 亮度，供软分箱提高 Y 直方图有效精度。
#[inline]
pub fn luma709_f32(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
}

/// RGB → 居中 Cb/Cr（约 [-128, 127]，用于矢量示波器）。
#[inline]
pub fn rgb_to_cb_cr(r: u8, g: u8, b: u8) -> (i8, i8) {
    let rf = r as f32;
    let gf = g as f32;
    let bf = b as f32;
    // BT.709 full-range style chroma (centered).
    let cb = (-0.114572 * rf - 0.385428 * gf + 0.500000 * bf).round();
    let cr = (0.500000 * rf - 0.454153 * gf - 0.045847 * bf).round();
    (
        cb.clamp(-128.0, 127.0) as i8,
        cr.clamp(-128.0, 127.0) as i8,
    )
}

/// 将 Cb/Cr 映射到 `[0, size)` 密度图坐标（中心为 0,0）。
#[inline]
pub fn cb_cr_to_scope_xy(cb: i8, cr: i8, size: u32) -> (u32, u32) {
    let half = (size as f32 - 1.0) * 0.5;
    let x = ((cb as f32 / 128.0) * half + half).round().clamp(0.0, size as f32 - 1.0) as u32;
    // Cr 向上为正（示波器习惯：上方偏红）。
    let y = ((-cr as f32 / 128.0) * half + half).round().clamp(0.0, size as f32 - 1.0) as u32;
    (x, y)
}

/// 肤色线角度（约 123°，相对 +Cb 轴），用于 vectorscope 参考线。
pub const SKIN_TONE_ANGLE_DEG: f32 = 123.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_and_white_luma() {
        assert_eq!(luma709(0, 0, 0), 0);
        assert_eq!(luma709(255, 255, 255), 255);
    }

    #[test]
    fn pure_red_chroma_quadrant() {
        let (cb, cr) = rgb_to_cb_cr(255, 0, 0);
        assert!(cr > 40, "red should have positive Cr, got {cr}");
        assert!(cb < 0, "red should have negative Cb, got {cb}");
    }

    #[test]
    fn pure_blue_chroma_quadrant() {
        let (cb, cr) = rgb_to_cb_cr(0, 0, 255);
        assert!(cb > 40, "blue should have positive Cb, got {cb}");
        assert!(cr < 20, "blue Cr should be low/negative, got {cr}");
    }

    #[test]
    fn scope_xy_center_for_neutral() {
        let (cb, cr) = rgb_to_cb_cr(128, 128, 128);
        let (x, y) = cb_cr_to_scope_xy(cb, cr, 256);
        assert!((x as i32 - 128).abs() < 3);
        assert!((y as i32 - 128).abs() < 3);
    }
}
