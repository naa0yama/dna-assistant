//! Result screen detection via OCR ("依頼完了" text recognition).
//!
//! Uses the [`OcrEngine`] trait to recognise text in the result screen ROI.
//! The detector crops the ROI from the frame, delegates OCR to the engine,
//! and checks whether the recognised text contains "依頼完了".

use std::time::Instant;

use image::RgbaImage;
use tracing::{Span, debug, instrument};

use crate::config::ResultScreenRoiConfig;
use crate::event::DetectionEvent;
use crate::ocr::OcrEngine;

/// Detects the result screen by OCR-scanning the "依頼完了" text area.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct ResultScreenDetector {
    config: ResultScreenRoiConfig,
}

impl ResultScreenDetector {
    /// Create a new result screen detector with the given ROI configuration.
    #[must_use]
    pub const fn new(config: ResultScreenRoiConfig) -> Self {
        Self { config }
    }

    /// Analyze a frame for the result screen using the given OCR engine.
    ///
    /// Crops the text ROI, delegates recognition to `ocr`, and returns
    /// `ResultScreenVisible` if "依頼終了" is found,
    /// `ResultScreenGone` otherwise.
    #[instrument(
        skip_all,
        name = "result_detect",
        fields(result.ocr_text, result.is_visible)
    )]
    pub fn analyze(&self, frame: &RgbaImage, ocr: &dyn OcrEngine) -> Vec<DetectionEvent> {
        let Some(roi_image) = self.config.text.crop(frame) else {
            return Vec::new();
        };

        let now = Instant::now();
        let span = Span::current();

        match ocr.recognize(&roi_image) {
            Ok(text) => {
                let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                // Match "終了" — the ROI targets the footer bar where only
                // "依頼終了" ends with 終了. Relaxed from "依頼終了" because
                // OCR sometimes misreads 頼 as 頤.
                let is_visible = normalized.contains("終了");

                span.record("result.ocr_text", &text);
                span.record("result.is_visible", is_visible);

                if is_visible {
                    vec![DetectionEvent::ResultScreenVisible {
                        text,
                        timestamp: now,
                    }]
                } else {
                    if !text.is_empty() {
                        debug!(ocr_text = %text, "result screen OCR: no match");
                    }
                    vec![DetectionEvent::ResultScreenGone { timestamp: now }]
                }
            }
            Err(e) => {
                debug!(error = %e, "result screen OCR failed");
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

    /// Stub OCR engine that always returns a fixed string.
    struct StubOcr(String);

    impl OcrEngine for StubOcr {
        fn recognize(&self, _image: &RgbaImage) -> Result<String, String> {
            Ok(self.0.clone())
        }
    }

    /// Stub OCR engine that always fails.
    struct FailOcr;

    impl OcrEngine for FailOcr {
        fn recognize(&self, _image: &RgbaImage) -> Result<String, String> {
            Err("OCR unavailable".into())
        }
    }

    fn test_config() -> ResultScreenRoiConfig {
        ResultScreenRoiConfig {
            text: RoiDefinition {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        }
    }

    #[test]
    fn visible_when_keyword_found() {
        let detector = ResultScreenDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr("依頼終了".into());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            DetectionEvent::ResultScreenVisible { .. }
        ));
    }

    #[test]
    fn visible_when_keyword_with_surrounding() {
        // Real OCR: "リトライ Esc 依頼終了"
        let detector = ResultScreenDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr("リ ト ラ イ Esc 依 頼 終 了".into());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            DetectionEvent::ResultScreenVisible { .. }
        ));
    }

    #[test]
    fn gone_when_text_does_not_match() {
        let detector = ResultScreenDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr("リトライ".into());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::ResultScreenGone { .. }));
    }

    #[test]
    fn gone_when_text_is_empty() {
        let detector = ResultScreenDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let ocr = StubOcr(String::new());
        let events = detector.analyze(&frame, &ocr);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DetectionEvent::ResultScreenGone { .. }));
    }

    #[test]
    fn empty_when_ocr_fails() {
        let detector = ResultScreenDetector::new(test_config());
        let frame = RgbaImage::new(10, 10);
        let events = detector.analyze(&frame, &FailOcr);
        assert!(events.is_empty());
    }
}
