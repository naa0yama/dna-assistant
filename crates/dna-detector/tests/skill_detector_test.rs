//! Integration tests for `SkillDetector` using real game frame ROI fixtures.
//!
//! Fixtures are cropped Q icon regions (80x41 RGBA) from actual gameplay.
//! Tests cover: skill active (ON), skill off (OFF), and SP-depleted (greyed out).

use dna_detector::config::SkillDetectorConfig;
use dna_detector::detector::Detector;
use dna_detector::detector::skill::SkillDetector;
use dna_detector::event::DetectionEvent;
use dna_detector::roi::RoiDefinition;
use dna_detector::titlebar::crop_titlebar;

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!("{}/tests/fixtures/skill/{name}", env!("CARGO_MANIFEST_DIR"));
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Config with full-frame ROI (fixtures are already cropped to the icon area).
const fn fixture_config() -> SkillDetectorConfig {
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

const fn is_ready(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::SkillReady { .. }))
}

const fn is_greyed(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::SkillGreyed { .. }))
}

// --- READY: skill ON (active, showing "0") ---

#[test]
fn skill_on_s1_f60() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_on_s1_f60.png");
    assert!(is_ready(&detector.analyze(&img)));
}

#[test]
fn skill_on_s1_f70() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_on_s1_f70.png");
    assert!(is_ready(&detector.analyze(&img)));
}

#[test]
fn skill_on_s1_f85_dark_bg() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_on_s1_f85.png");
    assert!(is_ready(&detector.analyze(&img)));
}

#[test]
fn skill_on_s2_f10() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_on_s2_f10.png");
    assert!(is_ready(&detector.analyze(&img)));
}

#[test]
fn skill_on_s3_f20_exploration() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_on_s3_f20.png");
    assert!(is_ready(&detector.analyze(&img)));
}

// --- READY: skill OFF (not active, showing SP cost) ---

#[test]
fn skill_off_s1_f28() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_off_s1_f28.png");
    assert!(is_ready(&detector.analyze(&img)));
}

#[test]
fn skill_ready_before_depletion() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_on_s4_t45.png");
    assert!(is_ready(&detector.analyze(&img)));
}

#[test]
fn skill_ready_after_recovery() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("ready_off_s4_t55.png");
    assert!(is_ready(&detector.analyze(&img)));
}

// --- GREYED: SP depleted ---

#[test]
fn skill_greyed_sp_depleted_t49() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("greyed_s4_t49.png");
    assert!(is_greyed(&detector.analyze(&img)));
}

#[test]
fn skill_greyed_sp_depleted_t50() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("greyed_s4_t50.png");
    assert!(is_greyed(&detector.analyze(&img)));
}

#[test]
fn skill_greyed_sp_depleted_t51() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("greyed_s4_t51.png");
    assert!(is_greyed(&detector.analyze(&img)));
}

#[test]
fn skill_greyed_sp_recovering_t52() {
    let detector = SkillDetector::new(fixture_config());
    let img = load_fixture("greyed_s4_t52.png");
    assert!(is_greyed(&detector.analyze(&img)));
}

// --- Full-frame pipeline (crop_titlebar + analyze) ---

/// Config with real ROI ratios (for full-frame fixtures).
const fn default_skill_config() -> SkillDetectorConfig {
    SkillDetectorConfig {
        roi: RoiDefinition {
            x: 0.878,
            y: 0.880,
            width: 0.042,
            height: 0.038,
        },
        greyed_max_brightness: 140,
        icon_bright_threshold: 0.05,
        icon_brightness_min: 120,
    }
}

#[test]
fn fhd_pipeline_ready_on() {
    let detector = SkillDetector::new(default_skill_config());
    let raw = load_fixture("ready_on_fhd.png");
    let game = crop_titlebar(&raw);
    assert!(is_ready(&detector.analyze(&game)));
}

#[test]
fn fhd_pipeline_greyed() {
    let detector = SkillDetector::new(default_skill_config());
    let raw = load_fixture("greyed_fhd.png");
    let game = crop_titlebar(&raw);
    assert!(is_greyed(&detector.analyze(&game)));
}

#[test]
fn fhd_pipeline_ready_off() {
    let detector = SkillDetector::new(default_skill_config());
    let raw = load_fixture("ready_off_fhd.png");
    let game = crop_titlebar(&raw);
    assert!(is_ready(&detector.analyze(&game)));
}
