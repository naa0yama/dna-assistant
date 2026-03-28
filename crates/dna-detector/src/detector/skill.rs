//! Q skill SP depletion detection via icon greyout.
//!
//! When SP is exhausted, the Q skill icon becomes greyed out — max brightness
//! drops from ~255 to ~100 and all bright icon pixels disappear. This is a
//! reliable indicator that SP recovery (from party skills) has failed.

use std::time::Instant;

use image::RgbaImage;
use tracing::instrument;

use crate::config::SkillDetectorConfig;
use crate::event::DetectionEvent;

use super::Detector;

/// Detects whether the Q skill icon is greyed out (SP depleted).
///
/// A greyed-out icon has very low maximum brightness and no bright
/// icon pixels, indicating the skill cannot be used due to SP exhaustion.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct SkillDetector {
    config: SkillDetectorConfig,
}

impl SkillDetector {
    /// Create a new skill detector with the given configuration.
    #[must_use]
    pub const fn new(config: SkillDetectorConfig) -> Self {
        Self { config }
    }

    /// Compute the ratio of bright icon pixels and the maximum brightness.
    ///
    /// Returns `(icon_bright_ratio, max_brightness)`.
    #[must_use]
    pub fn icon_metrics(&self, roi_image: &RgbaImage) -> (f64, u8) {
        let total = roi_image.width().saturating_mul(roi_image.height());
        if total == 0 {
            return (0.0, 0);
        }

        let bright_min = self.config.icon_brightness_min;
        let mut max_brightness: u8 = 0;
        let mut bright_count: u32 = 0;

        for p in roi_image.pixels() {
            let r = p.0[0];
            let g = p.0[1];
            let b = p.0[2];

            // Average brightness (integer approximation)
            let avg = (u16::from(r)
                .saturating_add(u16::from(g))
                .saturating_add(u16::from(b)))
                / 3;

            #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
            let avg_u8 = avg.min(255) as u8;
            if avg_u8 > max_brightness {
                max_brightness = avg_u8;
            }
            if avg_u8 >= bright_min {
                bright_count = bright_count.saturating_add(1);
            }
        }

        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let ratio = f64::from(bright_count) / f64::from(total);
        (ratio, max_brightness)
    }
}

impl Detector for SkillDetector {
    #[instrument(skip_all, name = "skill_detect")]
    fn analyze(&self, frame: &RgbaImage) -> Vec<DetectionEvent> {
        let Some(roi_image) = self.config.roi.crop(frame) else {
            return Vec::new();
        };

        let (icon_bright_ratio, max_brightness) = self.icon_metrics(&roi_image);
        let now = Instant::now();

        let is_greyed = max_brightness < self.config.greyed_max_brightness
            && icon_bright_ratio < self.config.icon_bright_threshold;

        if is_greyed {
            vec![DetectionEvent::SkillGreyed {
                icon_bright_ratio,
                max_brightness,
                timestamp: now,
            }]
        } else {
            vec![DetectionEvent::SkillReady {
                icon_bright_ratio,
                max_brightness,
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

    fn test_config() -> SkillDetectorConfig {
        SkillDetectorConfig {
            roi: RoiDefinition {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            greyed_max_brightness: 140,
            icon_bright_threshold: 0.05,
            icon_brightness_min: 120,
        }
    }

    #[test]
    fn bright_icon_detected_as_ready() {
        let mut img = RgbaImage::new(10, 10);
        // Fill 30% with bright white (active icon strokes)
        for y in 0..3 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
            }
        }
        let detector = SkillDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::SkillReady { .. }));
    }

    #[test]
    fn dark_icon_detected_as_greyed() {
        let mut img = RgbaImage::new(10, 10);
        // Fill with very dim pixels (greyed out)
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([30, 30, 30, 255]);
        }
        let detector = SkillDetector::new(test_config());
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::SkillGreyed { .. }));
    }

    #[test]
    fn icon_metrics_all_black() {
        let img = RgbaImage::new(10, 10);
        let detector = SkillDetector::new(test_config());
        let (ratio, max_br) = detector.icon_metrics(&img);
        assert!(ratio < f64::EPSILON);
        assert_eq!(max_br, 0);
    }

    #[test]
    fn icon_metrics_mixed_brightness() {
        let mut img = RgbaImage::new(10, 10);
        // 20% bright, 80% dark
        for y in 0..2 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([200, 200, 200, 255]));
            }
        }
        let detector = SkillDetector::new(test_config());
        let (ratio, max_br) = detector.icon_metrics(&img);
        assert!((ratio - 0.2).abs() < 0.01);
        assert_eq!(max_br, 200);
    }

    #[test]
    fn greyed_threshold_boundary() {
        let mut img = RgbaImage::new(10, 10);
        // Max brightness exactly at threshold (120) → NOT greyed
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([120, 120, 120, 255]);
        }
        let detector = SkillDetector::new(test_config());
        let events = detector.analyze(&img);
        // max_brightness=120, threshold is < 120, so NOT greyed
        assert!(matches!(events[0], DetectionEvent::SkillReady { .. }));
    }
}
