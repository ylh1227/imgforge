//! 屏幕 ↔ 原图像素 ↔ 归一化坐标 转换（纯函数，可单测）。

pub use crate::review::domain::coords::{
    norm_rect_to_screen, norm_to_pixel, norm_to_screen, norm_to_screen_pos2, pixel_to_norm,
    screen_rect_to_norm, screen_to_norm, NormPoint, NormRect, PixelPoint, ScreenPoint, Vec2,
    ViewportTransform,
};

/// 屏幕像素坐标 → 原图像素坐标。
pub fn screen_to_image(
    screen: ScreenPoint,
    transform: &ViewportTransform,
    image_size: (u32, u32),
) -> PixelPoint {
    let norm = screen_to_norm(screen, transform);
    norm_to_pixel(norm, image_size)
}

/// 原图像素坐标 → 归一化 0~1 坐标。
pub fn image_to_normalized(pixel: PixelPoint, image_size: (u32, u32)) -> NormPoint {
    pixel_to_norm(pixel, image_size)
}

/// 归一化坐标 → 屏幕像素坐标。
pub fn normalized_to_screen(norm: NormPoint, transform: &ViewportTransform) -> ScreenPoint {
    norm_to_screen(norm, transform)
}

/// 屏幕坐标 → 归一化坐标（常用快捷路径）。
pub fn screen_to_normalized(screen: ScreenPoint, transform: &ViewportTransform) -> NormPoint {
    screen_to_norm(screen, transform)
}

/// 以 `anchor` 为锚点缩放视口（滚轮缩放核心）。
pub fn zoom_at(
    transform: &mut ViewportTransform,
    factor: f32,
    anchor: ScreenPoint,
    min_zoom: f32,
    max_zoom: f32,
) {
    let before = screen_to_norm(anchor, transform);
    transform.zoom = (transform.zoom * factor).clamp(min_zoom, max_zoom);
    let after = norm_to_screen(before, transform);
    transform.pan.x += anchor.x - after.x;
    transform.pan.y += anchor.y - after.y;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_transform() -> ViewportTransform {
        ViewportTransform::fit_image(Vec2::new(800.0, 600.0), (1000, 500))
    }

    #[test]
    fn round_trip_normalized_screen() {
        let t = sample_transform();
        let norm = NormPoint { x: 0.25, y: 0.5 };
        let screen = normalized_to_screen(norm, &t);
        let back = screen_to_normalized(screen, &t);
        assert!((back.x - norm.x).abs() < 0.001);
        assert!((back.y - norm.y).abs() < 0.001);
    }

    #[test]
    fn round_trip_image_normalized() {
        let size = (1000u32, 500u32);
        let pixel = PixelPoint { x: 250, y: 125 };
        let norm = image_to_normalized(pixel, size);
        let back = norm_to_pixel(norm, size);
        assert_eq!(back.x, 250);
        assert_eq!(back.y, 125);
    }

    #[test]
    fn screen_image_round_trip() {
        let t = sample_transform();
        let size = (1000u32, 500u32);
        let screen = ScreenPoint { x: 200.0, y: 150.0 };
        let pixel = screen_to_image(screen, &t, size);
        let norm = image_to_normalized(pixel, size);
        let screen2 = normalized_to_screen(norm, &t);
        assert!((screen2.x - screen.x).abs() < 1.5);
        assert!((screen2.y - screen.y).abs() < 1.5);
    }

    #[test]
    fn zoom_at_keeps_anchor() {
        let mut t = sample_transform();
        let anchor = ScreenPoint { x: 400.0, y: 300.0 };
        let before = screen_to_normalized(anchor, &t);
        zoom_at(&mut t, 1.25, anchor, 0.1, 5.0);
        let after = screen_to_normalized(anchor, &t);
        assert!((after.x - before.x).abs() < 0.01);
        assert!((after.y - before.y).abs() < 0.01);
    }
}
