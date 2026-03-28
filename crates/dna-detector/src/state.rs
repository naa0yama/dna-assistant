//! Debounce wrapper for detectors to suppress transient state changes.

use std::time::{Duration, Instant};

use image::RgbaImage;
use tracing::instrument;

use crate::detector::Detector;
use crate::event::DetectionEvent;

/// Wraps a detector to suppress events during a cooldown period.
///
/// After an event is emitted, subsequent events of the same variant
/// are suppressed until the cooldown expires.
#[derive(Debug)]
pub struct DebouncedDetector<D> {
    inner: D,
    cooldown: Duration,
    last_event_time: Option<Instant>,
}

impl<D: Detector> DebouncedDetector<D> {
    /// Create a debounced wrapper with the specified cooldown duration.
    #[must_use]
    pub const fn new(inner: D, cooldown: Duration) -> Self {
        Self {
            inner,
            cooldown,
            last_event_time: None,
        }
    }

    /// Analyze a frame, returning events only if the cooldown has expired.
    #[instrument(skip_all, name = "debounced_process")]
    pub fn process(&mut self, frame: &RgbaImage) -> Vec<DetectionEvent> {
        let now = Instant::now();
        if let Some(last) = self.last_event_time
            && now.duration_since(last) < self.cooldown
        {
            return Vec::new();
        }

        let events = self.inner.analyze(frame);
        if !events.is_empty() {
            self.last_event_time = Some(now);
        }
        events
    }

    /// Reset the cooldown timer, allowing the next event through immediately.
    pub const fn reset(&mut self) {
        self.last_event_time = None;
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::SkillDetectorConfig;
    use crate::detector::skill::SkillDetector;
    use crate::roi::RoiDefinition;

    fn test_detector() -> SkillDetector {
        SkillDetector::new(SkillDetectorConfig {
            roi: RoiDefinition {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            greyed_max_brightness: 140,
            icon_bright_threshold: 0.05,
            icon_brightness_min: 120,
        })
    }

    fn bright_frame() -> RgbaImage {
        let mut img = RgbaImage::new(10, 10);
        for y in 0..3 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
            }
        }
        img
    }

    #[test]
    fn first_event_passes_through() {
        let mut debounced = DebouncedDetector::new(test_detector(), Duration::from_secs(1));
        let events = debounced.process(&bright_frame());
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn second_event_within_cooldown_suppressed() {
        let mut debounced = DebouncedDetector::new(test_detector(), Duration::from_secs(10));
        let events1 = debounced.process(&bright_frame());
        assert_eq!(events1.len(), 1);

        // Second call within cooldown should be suppressed
        let events2 = debounced.process(&bright_frame());
        assert!(events2.is_empty());
    }

    #[test]
    fn event_after_cooldown_passes_through() {
        let mut debounced = DebouncedDetector::new(test_detector(), Duration::from_millis(0));
        let events1 = debounced.process(&bright_frame());
        assert_eq!(events1.len(), 1);

        // Zero cooldown — next event should pass
        let events2 = debounced.process(&bright_frame());
        assert_eq!(events2.len(), 1);
    }

    #[test]
    fn reset_allows_immediate_event() {
        let mut debounced = DebouncedDetector::new(test_detector(), Duration::from_secs(10));
        let _ = debounced.process(&bright_frame());

        // Within cooldown, suppressed
        assert!(debounced.process(&bright_frame()).is_empty());

        // Reset, then event passes
        debounced.reset();
        let events = debounced.process(&bright_frame());
        assert_eq!(events.len(), 1);
    }
}
