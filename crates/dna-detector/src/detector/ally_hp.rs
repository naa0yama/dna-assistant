//! Ally HP detection via HP bar color area ratio.

use std::time::Instant;

use image::RgbaImage;
use tracing::instrument;

use crate::color::pixel_matches_hsv_range;
use crate::config::AllyHpDetectorConfig;
use crate::event::DetectionEvent;

use super::Detector;

/// Detects whether an ally is "down" by measuring the ratio
/// of HP-colored pixels in the ally status ROI.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct AllyHpDetector {
    config: AllyHpDetectorConfig,
    /// Which ally this detector monitors (0-indexed).
    ally_index: u8,
}

impl AllyHpDetector {
    /// Create a new ally HP detector for a specific ally.
    #[must_use]
    pub const fn new(config: AllyHpDetectorConfig, ally_index: u8) -> Self {
        Self { config, ally_index }
    }

    /// Compute the ratio of HP-colored pixels in a cropped ROI image.
    #[must_use]
    pub fn hp_ratio(&self, roi_image: &RgbaImage) -> f64 {
        let total = roi_image.width().saturating_mul(roi_image.height());
        if total == 0 {
            return 0.0;
        }
        let hp_count = roi_image
            .pixels()
            .filter(|p| pixel_matches_hsv_range(&p.0, &self.config.hp_color_range))
            .count();

        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let ratio = hp_count as f64 / f64::from(total);
        ratio
    }
}

impl Detector for AllyHpDetector {
    #[instrument(skip_all, name = "ally_hp_detect", fields(ally_index = self.ally_index))]
    fn analyze(&self, frame: &RgbaImage) -> Vec<DetectionEvent> {
        let Some(roi_image) = self.config.roi.crop(frame) else {
            return Vec::new();
        };
        let ratio = self.hp_ratio(&roi_image);
        let now = Instant::now();

        if ratio < self.config.down_threshold {
            vec![DetectionEvent::AllyHpLow {
                ally_index: self.ally_index,
                hp_ratio: ratio,
                timestamp: now,
            }]
        } else {
            vec![DetectionEvent::AllyHpNormal {
                ally_index: self.ally_index,
                hp_ratio: ratio,
                timestamp: now,
            }]
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::color::HsvRange;
    use crate::roi::RoiDefinition;

    fn test_config() -> AllyHpDetectorConfig {
        AllyHpDetectorConfig {
            roi: RoiDefinition {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            hp_color_range: HsvRange {
                h_min: 80.0,
                h_max: 150.0,
                s_min: 0.3,
                s_max: 1.0,
                v_min: 0.3,
                v_max: 1.0,
            },
            down_threshold: 0.05,
        }
    }

    #[test]
    fn full_hp_bar_detected_as_normal() {
        let mut img = RgbaImage::new(10, 10);
        // Fill with green (HP color): RGB(0, 200, 0)
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([0, 200, 0, 255]);
        }
        let detector = AllyHpDetector::new(test_config(), 0);
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::AllyHpNormal { .. }));
    }

    #[test]
    fn empty_hp_bar_detected_as_low() {
        let img = RgbaImage::new(10, 10); // all black
        let detector = AllyHpDetector::new(test_config(), 0);
        let events = detector.analyze(&img);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::AllyHpLow { .. }));
    }
}
