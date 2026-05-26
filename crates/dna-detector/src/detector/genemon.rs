//! Genemon liberation quest detection via OCR.
//!
//! Scans the quest ROI for the text "ジェネモン" or "解放". When either keyword
//! is found the quest is considered active; otherwise the quest is absent.
//! When OCR is unavailable the detector returns `GenemonGone`.

use std::time::Instant;

use image::RgbaImage;
use tracing::{Span, debug, instrument};

use crate::config::GenemonDetectorConfig;
use crate::event::DetectionEvent;
use crate::ocr::OcrEngine;

/// Detects the genemon liberation quest by OCR-scanning the quest text area.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct GenemonDetector {
    config: GenemonDetectorConfig,
}

impl GenemonDetector {
    /// Create a new genemon detector with the given configuration.
    #[must_use]
    pub const fn new(config: GenemonDetectorConfig) -> Self {
        Self { config }
    }

    /// Analyze a frame for the genemon quest using the given OCR engine.
    ///
    /// Returns `GenemonVisible` when "ジェネモン" or "解放" is found in the ROI,
    /// `GenemonGone` otherwise. Returns an empty vec when OCR fails.
    #[instrument(
        skip_all,
        name = "genemon_detect",
        fields(genemon.ocr_text, genemon.is_visible)
    )]
    pub fn analyze(&self, frame: &RgbaImage, ocr: &dyn OcrEngine) -> Vec<DetectionEvent> {
        let Some(roi_image) = self.config.quest_roi.crop(frame) else {
            return Vec::new();
        };

        let now = Instant::now();
        let span = Span::current();

        match ocr.recognize(&roi_image) {
            Ok(text) => {
                let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                let is_visible = normalized.contains("ジェネモン") || normalized.contains("解放");

                span.record("genemon.ocr_text", &text);
                span.record("genemon.is_visible", is_visible);

                if is_visible {
                    vec![DetectionEvent::GenemonVisible { timestamp: now }]
                } else {
                    if !text.is_empty() {
                        debug!(ocr_text = %text, "genemon OCR: no match");
                    }
                    vec![DetectionEvent::GenemonGone { timestamp: now }]
                }
            }
            Err(e) => {
                debug!(error = %e, "genemon OCR failed");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::roi::RoiDefinition;

    struct StubOcr(String);

    impl OcrEngine for StubOcr {
        fn recognize(&self, _image: &RgbaImage) -> Result<String, String> {
            Ok(self.0.clone())
        }
    }

    struct FailOcr;

    impl OcrEngine for FailOcr {
        fn recognize(&self, _image: &RgbaImage) -> Result<String, String> {
            Err("OCR unavailable".into())
        }
    }

    fn test_config() -> GenemonDetectorConfig {
        GenemonDetectorConfig {
            quest_roi: RoiDefinition {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        }
    }

    #[test]
    fn visible_when_genemon_keyword_found() {
        let detector = GenemonDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr("ジェネモンを探して解放しよう".into());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::GenemonVisible { .. }));
    }

    #[test]
    fn visible_when_liberation_keyword_found() {
        let detector = GenemonDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr("解放".into());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::GenemonVisible { .. }));
    }

    #[test]
    fn gone_when_no_keyword() {
        let detector = GenemonDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr("クエスト受注中".into());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::GenemonGone { .. }));
    }

    #[test]
    fn gone_when_text_is_empty() {
        let detector = GenemonDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr(String::new());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::GenemonGone { .. }));
    }

    #[test]
    fn empty_when_ocr_fails() {
        let detector = GenemonDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let events = detector.analyze(&frame, &FailOcr);
        assert!(events.is_empty());
    }
}
