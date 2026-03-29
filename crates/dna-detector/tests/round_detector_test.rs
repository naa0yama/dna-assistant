//! Integration tests for `RoundDetector` using real game frame ROI fixtures.
//!
//! Fixtures are cropped ROI regions (`RgbaImage`) from actual gameplay
//! across three visual filters: NORMAL, CINEMATIQ, Professional.

use dna_detector::config::RoundDetectorConfig;
use dna_detector::detector::Detector;
use dna_detector::detector::round::RoundDetector;
use dna_detector::event::DetectionEvent;
use dna_detector::roi::RoiDefinition;
use dna_detector::titlebar::crop_titlebar;

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!("{}/tests/fixtures/round/{name}", env!("CARGO_MANIFEST_DIR"));
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Config with full-frame ROI (fixtures are already cropped to ROI).
const fn roi_fixture_config() -> RoundDetectorConfig {
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

/// Config with real ROI ratios (for full-frame / titlebar-cropped fixtures).
const fn default_round_config() -> RoundDetectorConfig {
    RoundDetectorConfig {
        roi: RoiDefinition {
            x: 0.0,
            y: 0.256,
            width: 0.250,
            height: 0.035,
        },
        text_presence_threshold: 0.03,
        brightness_min: 140,
        max_chroma: 60,
        text_left_brightness_min: 200,
    }
}

const fn is_visible(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::RoundVisible { .. }))
}

// --- Text visible (gameplay) ---

#[cfg_attr(miri, ignore)]
#[test]
fn normal_filter_dark_background() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("normal_visible_dark.png");
    assert!(is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn normal_filter_bright_combat() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("normal_visible_bright.png");
    assert!(is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn cinematiq_filter_visible() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("cinematiq_visible.png");
    assert!(is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn pro_filter_bright_background() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("pro_visible_bright.png");
    assert!(is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn pro_s1_visible() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("pro_s1_visible.png");
    assert!(is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn pro_s1_extreme_lightning_still_visible() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("pro_s1_visible_lightning.png");
    assert!(is_visible(&detector.analyze(&img)));
}

// --- Text gone (result/cutscene) ---

#[cfg_attr(miri, ignore)]
#[test]
fn normal_filter_result_screen() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("normal_gone_result.png");
    assert!(!is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn pro_filter_result_screen() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("pro_gone_result.png");
    assert!(!is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn pro_s1_cutscene() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("pro_s1_gone_cutscene.png");
    assert!(!is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn pro_s1_round_end_screen() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("pro_s1_gone_round_end.png");
    assert!(!is_visible(&detector.analyze(&img)));
}

// --- 720p ROI-only fixtures ---

#[cfg_attr(miri, ignore)]
#[test]
fn hd_720p_visible_r23_roi() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("720p_visible_r23_roi.png");
    assert!(is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn hd_720p_visible_r24_roi() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("720p_visible_r24_roi.png");
    assert!(is_visible(&detector.analyze(&img)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn hd_720p_gone_round_end_roi() {
    let detector = RoundDetector::new(roi_fixture_config());
    let img = load_fixture("720p_gone_round_end_roi.png");
    assert!(!is_visible(&detector.analyze(&img)));
}

// --- 720p full-frame with title bar (crop_titlebar + analyze pipeline) ---

#[cfg_attr(miri, ignore)]
#[test]
fn hd_720p_titlebar_pipeline_visible() {
    let detector = RoundDetector::new(default_round_config());
    let raw = load_fixture("720p_visible_r23.png");
    let game = crop_titlebar(&raw);
    assert!(is_visible(&detector.analyze(&game)));
}

#[cfg_attr(miri, ignore)]
#[test]
fn hd_720p_titlebar_pipeline_gone() {
    let detector = RoundDetector::new(default_round_config());
    let raw = load_fixture("720p_gone_round_end.png");
    let game = crop_titlebar(&raw);
    assert!(!is_visible(&detector.analyze(&game)));
}
