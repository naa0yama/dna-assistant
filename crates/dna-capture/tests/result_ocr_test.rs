//! End-to-end OCR tests for `ResultScreenDetector` using real `JapaneseOcrEngine`.
//!
//! These tests run only on Windows (where `JapaneseOcrEngine` is available).
//! Fixtures are loaded from `dna-detector/tests/fixtures/result/` and contain
//! the result screen ROI area with surrounding pixels masked to black.

#[cfg(target_os = "windows")]
mod result_ocr {
    use dna_capture::ocr::JapaneseOcrEngine;
    use dna_detector::config::ResultScreenRoiConfig;
    use dna_detector::detector::result::ResultScreenDetector;
    use dna_detector::event::DetectionEvent;
    use dna_detector::titlebar::crop_titlebar;

    #[allow(clippy::panic)]
    fn load_fixture(name: &str) -> image::RgbaImage {
        let path = format!(
            "{}/../dna-detector/tests/fixtures/result/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        image::open(&path)
            .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
            .to_rgba8()
    }

    /// `result_1600x900.png` — normal completion screen, must return `ResultScreenVisible`.
    #[test]
    fn result_fixture_recognized_as_result_screen() {
        let ocr = JapaneseOcrEngine::new().expect("Japanese OCR unavailable");
        let raw = load_fixture("result_1600x900.png");
        let frame = crop_titlebar(&raw);
        let config = ResultScreenRoiConfig::default();
        let detector = ResultScreenDetector::new(config);
        let events = detector.analyze(&frame, &ocr);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DetectionEvent::ResultScreenVisible { .. })),
            "expected ResultScreenVisible, got {events:?}"
        );
    }

    /// `retry_1600x900.png` — retry screen also shows "依頼終了", must return `ResultScreenVisible`.
    #[test]
    fn retry_fixture_recognized_as_result_screen() {
        let ocr = JapaneseOcrEngine::new().expect("Japanese OCR unavailable");
        let raw = load_fixture("retry_1600x900.png");
        let frame = crop_titlebar(&raw);
        let config = ResultScreenRoiConfig::default();
        let detector = ResultScreenDetector::new(config);
        let events = detector.analyze(&frame, &ocr);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DetectionEvent::ResultScreenVisible { .. })),
            "expected ResultScreenVisible, got {events:?}"
        );
    }
}
