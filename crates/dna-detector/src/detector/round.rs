//! Round text detection via high-brightness, low-chroma pixel ratio.
//!
//! The round indicator ("探検 現在のラウンド：XX") uses neutral white/gray
//! text. Combat effects are bright but colorful. By requiring both high
//! brightness AND low chroma we reliably separate text from effects.

use std::time::Instant;

use image::RgbaImage;
use tracing::{Span, instrument};

use crate::color::text_pixel_ratio;
use crate::config::RoundDetectorConfig;
use crate::event::DetectionEvent;

use super::Detector;

/// Detects round text presence/absence by measuring text-like pixel density.
///
/// A "text pixel" is one with average brightness above `brightness_min`
/// and chroma (max(R,G,B) - min(R,G,B)) below `max_chroma`.
/// This filters out bright but colorful combat effects.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct RoundDetector {
    config: RoundDetectorConfig,
}

impl RoundDetector {
    /// Create a new round detector with the given configuration.
    #[must_use]
    pub const fn new(config: RoundDetectorConfig) -> Self {
        Self { config }
    }

    /// Compute the ratio of text-like pixels in a cropped ROI image.
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

    /// Check if the left portion of the ROI contains bright white text.
    ///
    /// The round text "探検 現在のラウンド：XX" always starts from the left
    /// edge with white characters (max brightness ~255). Result screens and
    /// other non-text backgrounds never reach this brightness in the left area.
    #[must_use]
    pub fn has_bright_text_left(&self, roi_image: &RgbaImage) -> bool {
        let quarter_w = roi_image.width() / 4;
        if quarter_w == 0 {
            return false;
        }

        let mut max_brightness: u8 = 0;
        for y in 0..roi_image.height() {
            for x in 0..quarter_w {
                let p = roi_image.get_pixel(x, y).0;
                let avg = (u16::from(p[0])
                    .saturating_add(u16::from(p[1]))
                    .saturating_add(u16::from(p[2])))
                    / 3;
                #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
                let avg_u8 = avg.min(255) as u8;
                if avg_u8 > max_brightness {
                    max_brightness = avg_u8;
                }
            }
        }

        max_brightness >= self.config.text_left_brightness_min
    }
}

impl Detector for RoundDetector {
    #[instrument(
        skip_all,
        name = "round_detect",
        fields(round.text_ratio, round.has_bright_left, round.is_visible)
    )]
    fn analyze(&self, frame: &RgbaImage) -> Vec<DetectionEvent> {
        let Some(roi_image) = self.config.roi.crop(frame) else {
            return Vec::new();
        };
        let ratio = self.text_ratio(&roi_image);
        let has_bright_left = self.has_bright_text_left(&roi_image);
        let has_text = ratio >= self.config.text_presence_threshold && has_bright_left;
        let now = Instant::now();

        let span = Span::current();
        span.record("round.text_ratio", ratio);
        span.record("round.has_bright_left", has_bright_left);
        span.record("round.is_visible", has_text);

        if has_text {
            vec![DetectionEvent::RoundVisible {
                text_present: true,
                white_ratio: ratio,
                timestamp: now,
            }]
        } else {
            vec![DetectionEvent::RoundGone {
                white_ratio: ratio,
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

    fn test_config() -> RoundDetectorConfig {
        RoundDetectorConfig {
            roi: RoiDefinition {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            text_presence_threshold: 0.03,
            brightness_min: 140,
            max_chroma: 60,
            text_left_brightness_min: 200,
        }
    }

    #[test]
    fn white_text_detected_as_visible() {
        let mut img = RgbaImage::new(10, 10);
        // Fill 20% with neutral white (high brightness, low chroma)
        for y in 0..2 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([200, 200, 200, 255]));
            }
        }
        let detector = RoundDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::RoundVisible { .. }));
    }

    #[test]
    fn dark_screen_detected_as_gone() {
        let img = RgbaImage::new(10, 10); // all black
        let detector = RoundDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::RoundGone { .. }));
    }

    #[test]
    fn bright_colorful_pixels_not_counted_as_text() {
        let mut img = RgbaImage::new(10, 10);
        // Fill 50% with bright magenta (high brightness, HIGH chroma)
        for y in 0..5 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([255, 50, 255, 255]));
            }
        }
        let detector = RoundDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        // Magenta is bright but colorful => should NOT be detected as text
        assert!(matches!(events[0], DetectionEvent::RoundGone { .. }));
    }

    #[test]
    fn text_ratio_calculation() {
        let mut img = RgbaImage::new(10, 10);
        // Fill first 3 rows (30 pixels / 100 = 0.30) with neutral light gray
        for y in 0..3 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([180, 180, 180, 255]));
            }
        }
        let detector = RoundDetector::new(test_config());
        let ratio = detector.text_ratio(&img);
        assert!((ratio - 0.3).abs() < 0.01);
    }

    #[test]
    fn mixed_text_and_effects_only_counts_text() {
        let mut img = RgbaImage::new(10, 10);
        // 10% neutral white (text-like)
        for x in 0..10 {
            img.put_pixel(x, 0, image::Rgba([210, 210, 210, 255]));
        }
        // 30% bright orange (combat effect - high chroma)
        for y in 1..4 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([255, 160, 30, 255]));
            }
        }
        // rest: dark background
        let detector = RoundDetector::new(test_config());
        let ratio = detector.text_ratio(&img);
        // Only the 10% neutral white should count
        assert!((ratio - 0.10).abs() < 0.02);
    }
}
