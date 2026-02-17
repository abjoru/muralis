use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use image::ImageReader;
use std::io::Cursor;

/// Compute the crop rectangle that a fill algorithm would use.
/// Returns (x, y, width, height) of the region kept visible.
pub fn compute_crop_rect(img_w: u32, img_h: u32, mon_w: u32, mon_h: u32) -> (u32, u32, u32, u32) {
    let img_ratio = img_w as f64 / img_h as f64;
    let mon_ratio = mon_w as f64 / mon_h as f64;

    if img_ratio > mon_ratio {
        // Image wider than monitor: crop sides
        let visible_w = (mon_ratio * img_h as f64).round() as u32;
        let x = (img_w.saturating_sub(visible_w)) / 2;
        (x, 0, visible_w.min(img_w), img_h)
    } else {
        // Image taller than monitor: crop top/bottom
        let visible_h = (img_w as f64 / mon_ratio).round() as u32;
        let y = (img_h.saturating_sub(visible_h)) / 2;
        (0, y, img_w, visible_h.min(img_h))
    }
}

/// Generate a version of the image with cropped regions darkened.
/// `darken_factor` controls brightness of cropped areas (0.3 = 30% brightness).
pub fn generate_crop_overlay(
    image_bytes: &[u8],
    mon_w: u32,
    mon_h: u32,
    darken_factor: f32,
) -> Result<Vec<u8>> {
    let img = ImageReader::new(Cursor::new(image_bytes))
        .with_guessed_format()?
        .decode()?;

    let mut rgba = img.to_rgba8();
    let (iw, ih) = (rgba.width(), rgba.height());
    let (cx, cy, cw, ch) = compute_crop_rect(iw, ih, mon_w, mon_h);

    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        let inside = x >= cx && x < cx + cw && y >= cy && y < cy + ch;
        if !inside {
            pixel[0] = (pixel[0] as f32 * darken_factor) as u8;
            pixel[1] = (pixel[1] as f32 * darken_factor) as u8;
            pixel[2] = (pixel[2] as f32 * darken_factor) as u8;
        }
    }

    let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut buf, 85);
    rgb.write_with_encoder(encoder)?;
    Ok(buf)
}

/// Returns true if image and monitor aspect ratios match within tolerance.
pub fn ratios_match(img_w: u32, img_h: u32, mon_w: u32, mon_h: u32, tolerance: f64) -> bool {
    if img_h == 0 || mon_h == 0 {
        return true;
    }
    let img_ratio = img_w as f64 / img_h as f64;
    let mon_ratio = mon_w as f64 / mon_h as f64;
    (img_ratio - mon_ratio).abs() < tolerance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_aspect_ratio_no_crop() {
        // 1920x1080 image on 1920x1080 monitor = full image visible
        let (x, y, w, h) = compute_crop_rect(1920, 1080, 1920, 1080);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
    }

    #[test]
    fn ultrawide_image_on_16_9_crops_sides() {
        // 3440x1440 (21:9) on 1920x1080 (16:9) — sides cropped
        let (x, y, w, h) = compute_crop_rect(3440, 1440, 1920, 1080);
        assert_eq!(y, 0);
        assert_eq!(h, 1440);
        // visible width = 1920/1080 * 1440 = 2560
        assert_eq!(w, 2560);
        assert_eq!(x, (3440 - 2560) / 2);
    }

    #[test]
    fn tall_image_on_16_9_crops_top_bottom() {
        // 1920x2560 (3:4) on 1920x1080 (16:9) — top/bottom cropped
        let (x, y, w, h) = compute_crop_rect(1920, 2560, 1920, 1080);
        assert_eq!(x, 0);
        assert_eq!(w, 1920);
        // visible height = 1920 / (1920/1080) = 1080
        assert_eq!(h, 1080);
        assert_eq!(y, (2560 - 1080) / 2);
    }

    #[test]
    fn ratios_match_same() {
        assert!(ratios_match(1920, 1080, 2560, 1440, 0.01));
    }

    #[test]
    fn ratios_differ() {
        assert!(!ratios_match(3440, 1440, 1920, 1080, 0.01));
    }

    #[test]
    fn overlay_produces_valid_jpeg() {
        // Create a tiny test image
        let img = image::RgbImage::from_pixel(100, 100, image::Rgb([128, 128, 128]));
        let mut bytes = Vec::new();
        let encoder = JpegEncoder::new_with_quality(&mut bytes, 85);
        img.write_with_encoder(encoder).unwrap();

        let result = generate_crop_overlay(&bytes, 1920, 1080, 0.3).unwrap();
        assert!(!result.is_empty());

        // Verify it decodes
        let decoded = ImageReader::new(Cursor::new(&result))
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(decoded.width(), 100);
        assert_eq!(decoded.height(), 100);
    }
}
