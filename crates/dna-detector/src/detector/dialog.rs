//! Dialog detection via dual-ROI analysis (text density + dark background).
//!
//! Centered game dialogs (e.g., "Tips" network error) overlay the screen with
//! a dark box containing white text. We detect them by checking:
//! 1. High density of low-chroma bright pixels in the text ROI (white text).
//! 2. High density of dark pixels in the background ROI (dialog backdrop).
//!
//! This dual check separates dialogs from both gameplay (bright but colorful
//! combat effects) and result screens (dark but no text in this region).

use std::time::Instant;

use image::RgbaImage;
use tracing::instrument;

use crate::color::text_pixel_ratio;
use crate::config::DialogDetectorConfig;
use crate::event::DetectionEvent;

use super::Detector;

/// Detects centered dialog boxes by combining text density and background darkness.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct DialogDetector {
    config: DialogDetectorConfig,
}

impl DialogDetector {
    /// Create a new dialog detector with the given configuration.
    #[must_use]
    pub const fn new(config: DialogDetectorConfig) -> Self {
        Self { config }
    }

    /// Compute the ratio of low-chroma bright pixels (text-like) in the text ROI.
    ///
    /// Delegates to [`text_pixel_ratio`] with this detector's thresholds.
    #[must_use]
    pub fn text_ratio(&self, roi_image: &RgbaImage) -> f64 {
        text_pixel_ratio(
            roi_image,
            self.config.brightness_min,
            self.config.max_chroma,
        )
    }

    /// Compute the ratio of dark pixels in the background ROI.
    ///
    /// A pixel is dark when average brightness (R+G+B)/3 <= `bg_brightness_max`.
    #[must_use]
    pub fn bg_dark_ratio(&self, roi_image: &RgbaImage) -> f64 {
        let total = roi_image.width().saturating_mul(roi_image.height());
        if total == 0 {
            return 0.0;
        }

        let max_bright = u16::from(self.config.bg_brightness_max);

        let dark_count = roi_image
            .pixels()
            .filter(|p| {
                let r = u16::from(p.0[0]);
                let g = u16::from(p.0[1]);
                let b = u16::from(p.0[2]);
                let avg = (r.saturating_add(g).saturating_add(b)) / 3;
                avg <= max_bright
            })
            .count();

        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let ratio = dark_count as f64 / f64::from(total);
        ratio
    }
}

impl Detector for DialogDetector {
    #[instrument(skip_all, name = "dialog_detect")]
    fn analyze(&self, frame: &RgbaImage) -> Vec<DetectionEvent> {
        // Check bg ROI first: cheaper (brightness-only) and more selective
        // (threshold 0.85), allowing early rejection of gameplay frames.
        let Some(bg_roi) = self.config.bg_roi.crop(frame) else {
            return Vec::new();
        };
        let bg_dark_ratio = self.bg_dark_ratio(&bg_roi);

        let text_ratio = if bg_dark_ratio >= self.config.bg_dark_threshold {
            let Some(text_roi) = self.config.text_roi.crop(frame) else {
                return Vec::new();
            };
            self.text_ratio(&text_roi)
        } else {
            0.0
        };

        let now = Instant::now();
        let is_dialog = text_ratio >= self.config.text_presence_threshold
            && bg_dark_ratio >= self.config.bg_dark_threshold;

        if is_dialog {
            vec![DetectionEvent::DialogVisible {
                text_ratio,
                bg_dark_ratio,
                timestamp: now,
            }]
        } else {
            vec![DetectionEvent::DialogGone {
                text_ratio,
                bg_dark_ratio,
                timestamp: now,
            }]
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::roi::RoiDefinition;

    fn test_config() -> DialogDetectorConfig {
        DialogDetectorConfig {
            text_roi: RoiDefinition {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 0.5,
            },
            bg_roi: RoiDefinition {
                x: 0.0,
                y: 0.5,
                width: 1.0,
                height: 0.5,
            },
            text_presence_threshold: 0.10,
            bg_dark_threshold: 0.85,
            brightness_min: 100,
            max_chroma: 60,
            bg_brightness_max: 25,
        }
    }

    #[test]
    fn dialog_detected_with_white_text_and_dark_bg() {
        let mut img = RgbaImage::new(10, 10);
        // Top half: 20% white text pixels (low chroma, high brightness)
        for x in 0..10 {
            img.put_pixel(x, 0, image::Rgba([200, 200, 200, 255]));
        }
        // Bottom half: all dark (100% dark)
        // (default is black)
        let detector = DialogDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::DialogVisible { .. }));
    }

    #[test]
    fn no_dialog_when_bg_not_dark() {
        let mut img = RgbaImage::new(10, 10);
        // Top half: white text
        for x in 0..10 {
            img.put_pixel(x, 0, image::Rgba([200, 200, 200, 255]));
        }
        // Bottom half: bright background (not dark)
        for y in 5..10 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([128, 128, 128, 255]));
            }
        }
        let detector = DialogDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::DialogGone { .. }));
    }

    #[test]
    fn no_dialog_when_no_text() {
        // All black — dark bg but no text
        let img = RgbaImage::new(10, 10);
        let detector = DialogDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::DialogGone { .. }));
    }

    #[test]
    fn colorful_bright_pixels_not_counted_as_text() {
        let mut img = RgbaImage::new(10, 10);
        // Top half: bright magenta (high chroma — combat effects)
        for y in 0..5 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([255, 50, 255, 255]));
            }
        }
        // Bottom half: dark
        let detector = DialogDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::DialogGone { .. }));
    }

    #[test]
    fn text_ratio_calculation() {
        let mut img = RgbaImage::new(10, 10);
        // 30 pixels (top 3 rows of top half = 30/50 = 0.60) neutral white
        for y in 0..3 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([180, 180, 180, 255]));
            }
        }
        let config = test_config();
        let detector = DialogDetector::new(config);
        let roi = RgbaImage::from_fn(10, 5, |x, y| *img.get_pixel(x, y));
        let ratio = detector.text_ratio(&roi);
        assert!((ratio - 0.6).abs() < 0.01);
    }

    #[test]
    fn bg_dark_ratio_calculation() {
        let mut img = RgbaImage::new(10, 10);
        // Bottom half: 4 dark rows + 1 bright row = 40/50 = 0.80
        for y in 5..9 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([10, 10, 10, 255]));
            }
        }
        for x in 0..10 {
            img.put_pixel(x, 9, image::Rgba([200, 200, 200, 255]));
        }
        let config = test_config();
        let detector = DialogDetector::new(config);
        let roi = RgbaImage::from_fn(10, 5, |x, y| *img.get_pixel(x, y + 5));
        let ratio = detector.bg_dark_ratio(&roi);
        assert!((ratio - 0.8).abs() < 0.01);
    }
}
