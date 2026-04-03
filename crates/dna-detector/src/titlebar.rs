//! Windows title bar detection and removal.
//!
//! Window captures (WGC, `PrintWindow`) include the title bar.
//! This module detects it and returns the game client area.

use image::RgbaImage;

/// Detect the height of a Windows title bar at the top of a captured frame.
///
/// Returns 0 if no title bar is detected (e.g. borderless/fullscreen capture
/// or client-area-only capture).
///
/// Detection criteria:
/// - Bright (mean > 150) and horizontally uniform (spread < 40)
///   across left, center, and right thirds of each row
/// - Must start within the first 3 rows (allowing for border/shadow pixels)
/// - Must be a contiguous band of qualifying rows
#[must_use]
pub fn detect_titlebar_height(frame: &RgbaImage) -> u32 {
    let (w, h) = (frame.width(), frame.height());
    let check_rows = h.min(50);
    let third = w / 3;

    if third == 0 {
        return 0;
    }

    let mut titlebar_started = false;
    let mut titlebar_end: u32 = 0;

    for y in 0..check_rows {
        let (left_sum, center_sum, right_sum) = row_third_sums(frame, y, third, w);

        let left_count = third;
        let center_count = third;
        #[allow(clippy::arithmetic_side_effects)]
        let right_count = w.saturating_sub(2 * third);

        if right_count == 0 {
            return 0;
        }

        #[allow(clippy::cast_precision_loss)]
        let left_mean = left_sum / (f64::from(left_count) * 3.0);
        #[allow(clippy::cast_precision_loss)]
        let center_mean = center_sum / (f64::from(center_count) * 3.0);
        #[allow(clippy::cast_precision_loss)]
        let right_mean = right_sum / (f64::from(right_count) * 3.0);

        let all_bright = left_mean > 150.0 && center_mean > 150.0 && right_mean > 150.0;
        let max_val = left_mean.max(center_mean).max(right_mean);
        let min_val = left_mean.min(center_mean).min(right_mean);
        let uniform = (max_val - min_val) < 40.0;

        if all_bright && uniform {
            if !titlebar_started {
                // Title bar must start within first 3 rows
                if y > 3 {
                    return 0;
                }
                titlebar_started = true;
            }
            titlebar_end = y.saturating_add(1);
        } else if titlebar_started {
            // End of contiguous bright band
            break;
        } else {
            // Not a title bar row and we haven't found one yet — keep looking
            // (border/shadow pixels at rows 0-3 are allowed)
            if y >= 3 {
                return 0;
            }
        }
    }

    titlebar_end
}

/// Crop the title bar from a frame, returning only the game client area.
///
/// If no title bar is detected, the original image is cloned.
#[allow(clippy::module_name_repetitions)]
#[must_use]
pub fn crop_titlebar(frame: &RgbaImage) -> RgbaImage {
    let tb = detect_titlebar_height(frame);
    if tb == 0 || tb >= frame.height() {
        return frame.clone();
    }
    image::imageops::crop_imm(
        frame,
        0,
        tb,
        frame.width(),
        frame.height().saturating_sub(tb),
    )
    .to_image()
}

/// Sum R+G+B for left, center, and right thirds of a row.
fn row_third_sums(frame: &RgbaImage, y: u32, third: u32, width: u32) -> (f64, f64, f64) {
    let mut left_sum: f64 = 0.0;
    let mut center_sum: f64 = 0.0;
    let mut right_sum: f64 = 0.0;

    for x in 0..width {
        let p = frame.get_pixel(x, y).0;
        let rgb_sum = f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2]);
        if x < third {
            left_sum += rgb_sum;
        } else if x < third.saturating_mul(2) {
            center_sum += rgb_sum;
        } else {
            right_sum += rgb_sum;
        }
    }

    (left_sum, center_sum, right_sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_titlebar_on_dark_frame() {
        // Pure dark frame — no title bar
        let img = RgbaImage::new(100, 100);
        assert_eq!(detect_titlebar_height(&img), 0);
    }

    #[test]
    fn detects_bright_uniform_top_band() {
        let mut img = RgbaImage::new(300, 100);
        // Fill top 30 rows with bright uniform white
        for y in 0..30 {
            for x in 0..300 {
                img.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
            }
        }
        let tb = detect_titlebar_height(&img);
        assert_eq!(tb, 30);
    }

    #[test]
    fn ignores_bright_band_not_at_top() {
        let mut img = RgbaImage::new(300, 100);
        // Bright band at rows 20-40, not at top
        for y in 20..40 {
            for x in 0..300 {
                img.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
            }
        }
        assert_eq!(detect_titlebar_height(&img), 0);
    }

    #[test]
    fn allows_dark_border_pixel_at_row_0() {
        let mut img = RgbaImage::new(300, 100);
        // Row 0: dark border
        // Rows 1-30: bright title bar
        for y in 1..31 {
            for x in 0..300 {
                img.put_pixel(x, y, image::Rgba([230, 230, 230, 255]));
            }
        }
        let tb = detect_titlebar_height(&img);
        assert_eq!(tb, 31);
    }

    #[test]
    fn non_uniform_bright_row_not_titlebar() {
        let mut img = RgbaImage::new(300, 100);
        // Left third bright, right third dark — not uniform
        for y in 0..30 {
            for x in 0..100 {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
            // center and right stay black
        }
        assert_eq!(detect_titlebar_height(&img), 0);
    }

    #[test]
    fn crop_titlebar_returns_game_area() {
        let mut img = RgbaImage::new(200, 100);
        // 30px title bar
        for y in 0..30 {
            for x in 0..200 {
                img.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
            }
        }
        // Game content: dark
        let game = crop_titlebar(&img);
        assert_eq!(game.width(), 200);
        assert_eq!(game.height(), 70);
    }

    #[test]
    fn crop_titlebar_noop_when_no_titlebar() {
        let img = RgbaImage::new(200, 100);
        let game = crop_titlebar(&img);
        assert_eq!(game.width(), 200);
        assert_eq!(game.height(), 100);
    }
}
