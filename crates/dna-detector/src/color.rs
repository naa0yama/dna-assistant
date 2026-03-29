//! RGB to HSV color space conversion and color matching utilities.

use serde::{Deserialize, Serialize};

/// HSV color representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    /// Hue in degrees (0.0..360.0).
    pub h: f64,
    /// Saturation (0.0..=1.0).
    pub s: f64,
    /// Value / brightness (0.0..=1.0).
    pub v: f64,
}

/// A range in HSV space for color matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HsvRange {
    /// Minimum hue in degrees.
    pub h_min: f64,
    /// Maximum hue in degrees.
    pub h_max: f64,
    /// Minimum saturation (0.0..=1.0).
    pub s_min: f64,
    /// Maximum saturation (0.0..=1.0).
    pub s_max: f64,
    /// Minimum value (0.0..=1.0).
    pub v_min: f64,
    /// Maximum value (0.0..=1.0).
    pub v_max: f64,
}

/// Convert an RGB pixel to HSV.
#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> Hsv {
    let rf = f64::from(r) / 255.0;
    let gf = f64::from(g) / 255.0;
    let bf = f64::from(b) / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;

    let h = if delta < f64::EPSILON {
        0.0
    } else if (max - rf).abs() < f64::EPSILON {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if (max - gf).abs() < f64::EPSILON {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };

    // Normalize negative hue to 0..360 range.
    let h = if h < 0.0 { h + 360.0 } else { h };

    let s = if max < f64::EPSILON { 0.0 } else { delta / max };

    Hsv { h, s, v: max }
}

/// Check if an RGBA pixel falls within an HSV range.
#[must_use]
pub fn pixel_matches_hsv_range(pixel: &[u8; 4], range: &HsvRange) -> bool {
    let hsv = rgb_to_hsv(pixel[0], pixel[1], pixel[2]);
    (range.h_min..=range.h_max).contains(&hsv.h)
        && (range.s_min..=range.s_max).contains(&hsv.s)
        && (range.v_min..=range.v_max).contains(&hsv.v)
}

/// Compute the ratio of low-chroma bright pixels (text-like) in an image.
///
/// A pixel is text-like when:
/// - average brightness (R+G+B)/3 >= `brightness_min`
/// - chroma max(R,G,B) - min(R,G,B) < `max_chroma`
#[must_use]
pub fn text_pixel_ratio(image: &image::RgbaImage, brightness_min: u8, max_chroma: u8) -> f64 {
    let total = image.width().saturating_mul(image.height());
    if total == 0 {
        return 0.0;
    }

    let bright_min = u16::from(brightness_min);
    let chroma_max = u16::from(max_chroma);

    let text_count = image
        .pixels()
        .filter(|p| {
            let r = u16::from(p.0[0]);
            let g = u16::from(p.0[1]);
            let b = u16::from(p.0[2]);
            let avg = (r.saturating_add(g).saturating_add(b)) / 3;
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            let chroma = max_c.saturating_sub(min_c);
            avg >= bright_min && chroma < chroma_max
        })
        .count();

    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let ratio = text_count as f64 / f64::from(total);
    ratio
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_red() {
        let hsv = rgb_to_hsv(255, 0, 0);
        assert!((hsv.h - 0.0).abs() < 1.0);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn pure_green() {
        let hsv = rgb_to_hsv(0, 255, 0);
        assert!((hsv.h - 120.0).abs() < 1.0);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn pure_blue() {
        let hsv = rgb_to_hsv(0, 0, 255);
        assert!((hsv.h - 240.0).abs() < 1.0);
        assert!((hsv.s - 1.0).abs() < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn white_has_zero_saturation() {
        let hsv = rgb_to_hsv(255, 255, 255);
        assert!(hsv.s < 0.01);
        assert!((hsv.v - 1.0).abs() < 0.01);
    }

    #[test]
    fn black_has_zero_value() {
        let hsv = rgb_to_hsv(0, 0, 0);
        assert!(hsv.v < 0.01);
    }

    #[test]
    fn gold_color_matches_range() {
        // Approximate gold: RGB(218, 165, 32)
        let pixel: [u8; 4] = [218, 165, 32, 255];
        let gold_range = HsvRange {
            h_min: 30.0,
            h_max: 50.0,
            s_min: 0.4,
            s_max: 1.0,
            v_min: 0.6,
            v_max: 1.0,
        };
        assert!(pixel_matches_hsv_range(&pixel, &gold_range));
    }

    #[test]
    fn blue_does_not_match_gold_range() {
        let pixel: [u8; 4] = [0, 0, 255, 255];
        let gold_range = HsvRange {
            h_min: 30.0,
            h_max: 50.0,
            s_min: 0.4,
            s_max: 1.0,
            v_min: 0.6,
            v_max: 1.0,
        };
        assert!(!pixel_matches_hsv_range(&pixel, &gold_range));
    }
}
