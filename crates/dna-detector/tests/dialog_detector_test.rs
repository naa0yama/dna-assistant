//! Integration tests for `DialogDetector` using real game frame fixtures.
//!
//! Fixtures are cropped to the combined ROI bounding box from actual
//! gameplay across dialog-visible and dialog-gone scenarios.

use dna_detector::config::DialogDetectorConfig;
use dna_detector::detector::Detector;
use dna_detector::detector::dialog::DialogDetector;
use dna_detector::event::DetectionEvent;
use dna_detector::roi::RoiDefinition;

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!(
        "{}/tests/fixtures/dialog/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Config with ROI positions adjusted for cropped fixtures.
///
/// Fixtures are cropped from full frame at:
///   x = 0.20..0.80 of frame width, y = 0.35..0.60 of game height.
/// ROI coordinates are remapped to this cropped coordinate space.
const fn fixture_config() -> DialogDetectorConfig {
    DialogDetectorConfig {
        text_roi: RoiDefinition {
            x: 0.1833,
            y: 0.2698,
            width: 0.6167,
            height: 0.1297,
        },
        bg_roi: RoiDefinition {
            x: 0.0833,
            y: 0.0536,
            width: 0.8333,
            height: 0.6486,
        },
        text_presence_threshold: 0.10,
        bg_dark_threshold: 0.85,
        brightness_min: 100,
        max_chroma: 60,
        bg_brightness_max: 25,
    }
}

const fn is_visible(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::DialogVisible { .. }))
}

const fn is_gone(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::DialogGone { .. }))
}

// --- Dialog visible ---

#[cfg_attr(miri, ignore)]
#[test]
fn visible_nw_error() {
    let detector = DialogDetector::new(fixture_config());
    let frame = load_fixture("visible_nw_error.png");
    let events = detector.analyze(&frame);
    assert!(
        is_visible(&events),
        "expected DialogVisible, got {events:?}"
    );
}

// --- Dialog gone ---

#[cfg_attr(miri, ignore)]
#[test]
fn gone_result_screen() {
    let detector = DialogDetector::new(fixture_config());
    let frame = load_fixture("gone_result_screen.png");
    let events = detector.analyze(&frame);
    assert!(is_gone(&events), "expected DialogGone, got {events:?}");
}

#[cfg_attr(miri, ignore)]
#[test]
fn gone_gameplay() {
    let detector = DialogDetector::new(fixture_config());
    let frame = load_fixture("gone_gameplay.png");
    let events = detector.analyze(&frame);
    assert!(is_gone(&events), "expected DialogGone, got {events:?}");
}
