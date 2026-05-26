//! End-to-end OCR tests for `GenemonDetector` using real `JapaneseOcrEngine`.
//!
//! These tests run only on Windows (where `JapaneseOcrEngine` is available).
//! The fixtures are loaded from `dna-detector/tests/fixtures/genemon/` and
//! contain the quest ROI area with surrounding pixels masked to black.

#[cfg(target_os = "windows")]
mod genemon_ocr {
    use dna_capture::ocr::JapaneseOcrEngine;
    use dna_detector::config::DetectionConfig;
    use dna_detector::detector::genemon::GenemonDetector;
    use dna_detector::event::DetectionEvent;
    use dna_detector::titlebar::crop_titlebar;

    #[allow(clippy::panic)]
    fn load_fixture(name: &str) -> image::RgbaImage {
        let path = format!(
            "{}/../dna-detector/tests/fixtures/genemon/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        image::open(&path)
            .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
            .to_rgba8()
    }

    /// `visible.png` contains the genemon quest text — OCR must return `GenemonVisible`.
    #[test]
    fn visible_fixture_recognized_as_genemon_quest() {
        let ocr = JapaneseOcrEngine::new().expect("Japanese OCR unavailable");
        let raw = load_fixture("visible.png");
        let frame = crop_titlebar(&raw);
        let config = DetectionConfig::default();
        let detector = GenemonDetector::new(config.genemon);
        let events = detector.analyze(&frame, &ocr);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DetectionEvent::GenemonVisible { .. })),
            "expected GenemonVisible, got {events:?}"
        );
    }

    /// `gone.png` contains unrelated quest text — OCR must return `GenemonGone`.
    #[test]
    fn gone_fixture_not_recognized_as_genemon_quest() {
        let ocr = JapaneseOcrEngine::new().expect("Japanese OCR unavailable");
        let raw = load_fixture("gone.png");
        let frame = crop_titlebar(&raw);
        let config = DetectionConfig::default();
        let detector = GenemonDetector::new(config.genemon);
        let events = detector.analyze(&frame, &ocr);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DetectionEvent::GenemonGone { .. })),
            "expected GenemonGone, got {events:?}"
        );
    }
}
