//! Ratio-based Region of Interest (ROI) that scales to any capture resolution.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

/// A region defined as ratios (0.0..=1.0) relative to the frame dimensions.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoiDefinition {
    /// Left edge ratio (0.0 = left, 1.0 = right).
    pub x: f64,
    /// Top edge ratio (0.0 = top, 1.0 = bottom).
    pub y: f64,
    /// Width ratio relative to frame width.
    pub width: f64,
    /// Height ratio relative to frame height.
    pub height: f64,
}

/// Pixel-coordinate rectangle computed from a ratio-based ROI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    /// X offset in pixels.
    pub x: u32,
    /// Y offset in pixels.
    pub y: u32,
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
}

impl RoiDefinition {
    /// Convert ratio-based ROI to pixel coordinates for a given frame size.
    #[must_use]
    pub fn to_pixels(&self, frame_width: u32, frame_height: u32) -> PixelRect {
        let fw = f64::from(frame_width);
        let fh = f64::from(frame_height);

        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions
        )]
        PixelRect {
            x: (self.x * fw) as u32,
            y: (self.y * fh) as u32,
            w: (self.width * fw) as u32,
            h: (self.height * fh) as u32,
        }
    }

    /// Crop the specified ROI region from a frame.
    ///
    /// Returns `None` if the ROI exceeds frame bounds.
    #[must_use]
    pub fn crop(&self, frame: &RgbaImage) -> Option<RgbaImage> {
        let rect = self.to_pixels(frame.width(), frame.height());
        if rect.x.saturating_add(rect.w) > frame.width()
            || rect.y.saturating_add(rect.h) > frame.height()
            || rect.w == 0
            || rect.h == 0
        {
            return None;
        }
        Some(image::imageops::crop_imm(frame, rect.x, rect.y, rect.w, rect.h).to_image())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_pixels_scales_correctly() {
        let roi = RoiDefinition {
            x: 0.1,
            y: 0.2,
            width: 0.5,
            height: 0.3,
        };
        let rect = roi.to_pixels(1920, 1080);
        assert_eq!(rect.x, 192);
        assert_eq!(rect.y, 216);
        assert_eq!(rect.w, 960);
        assert_eq!(rect.h, 324);
    }

    #[test]
    fn crop_returns_correct_size() {
        let frame = RgbaImage::new(100, 100);
        let roi = RoiDefinition {
            x: 0.1,
            y: 0.1,
            width: 0.5,
            height: 0.5,
        };
        let cropped = roi.crop(&frame).expect("crop should succeed");
        assert_eq!(cropped.width(), 50);
        assert_eq!(cropped.height(), 50);
    }

    #[test]
    fn crop_out_of_bounds_returns_none() {
        let frame = RgbaImage::new(100, 100);
        let roi = RoiDefinition {
            x: 0.8,
            y: 0.8,
            width: 0.5,
            height: 0.5,
        };
        assert!(roi.crop(&frame).is_none());
    }

    #[test]
    fn crop_zero_size_returns_none() {
        let frame = RgbaImage::new(100, 100);
        let roi = RoiDefinition {
            x: 0.5,
            y: 0.5,
            width: 0.0,
            height: 0.0,
        };
        assert!(roi.crop(&frame).is_none());
    }
}
