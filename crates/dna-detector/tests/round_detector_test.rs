//! Integration tests for `RoundDetector` using real game frame fixtures.
//!
//! Fixtures are full-frame captures (with titlebar) masked to config ROIs.
//! The detector pipeline: `crop_titlebar` → `RoundDetector::analyze`.

use dna_detector::config::{DetectionConfig, RoundDetectorConfig};
use dna_detector::detector::Detector;
use dna_detector::detector::round::RoundDetector;
use dna_detector::event::DetectionEvent;
use dna_detector::roi::{PixelRect, RoiDefinition};
use dna_detector::titlebar::crop_titlebar;

// ── Helpers ─────────────────────────────────────────────────────────

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!("{}/tests/fixtures/round/{name}", env!("CARGO_MANIFEST_DIR"));
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Save an image to the fixtures directory.
#[allow(clippy::panic)]
fn save_fixture(name: &str, image: &image::RgbaImage) {
    let dir = format!("{}/tests/fixtures/round", env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        panic!("failed to create output dir {dir}: {e}");
    }
    let path = format!("{dir}/{name}");
    image
        .save(&path)
        .unwrap_or_else(|e| panic!("failed to save {path}: {e}"));
}

/// Mask a full-frame fixture: black out everything outside the titlebar
/// and the given ROI regions (which are relative to the game frame).
fn mask_fixture(raw: &image::RgbaImage, rois: &[&RoiDefinition]) -> image::RgbaImage {
    let (w, h) = raw.dimensions();
    let game = crop_titlebar(raw);
    let titlebar_h = h.saturating_sub(game.height());

    let rects: Vec<PixelRect> = rois
        .iter()
        .map(|r| {
            let mut pr = r.to_pixels(game.width(), game.height());
            pr.y = pr.y.saturating_add(titlebar_h);
            pr
        })
        .collect();

    let mut out = raw.clone();
    let black = image::Rgba([0u8, 0, 0, 255]);
    for py in titlebar_h..h {
        for px in 0..w {
            let inside = rects.iter().any(|r| {
                px >= r.x
                    && px < r.x.saturating_add(r.w)
                    && py >= r.y
                    && py < r.y.saturating_add(r.h)
            });
            if !inside {
                out.put_pixel(px, py, black);
            }
        }
    }
    out
}

/// Config with real ROI ratios from config defaults.
fn default_round_config() -> RoundDetectorConfig {
    DetectionConfig::default().round
}

const fn is_visible(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::RoundVisible { .. }))
}

// ── Fixture mask + crop ─────────────────────────────────────────────

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn mask_round_visible_1600x900() {
    let config = default_round_config();
    let raw = load_fixture("visible_1600x900.png");

    let masked = mask_fixture(&raw, &[&config.roi]);
    save_fixture("visible_1600x900.png", &masked);

    let game = crop_titlebar(&masked);
    if let Some(crop) = config.roi.crop(&game) {
        save_fixture("roi_1600x900.png", &crop);
    }
}

// ── Detection tests ─────────────────────────────────────────────────

#[cfg_attr(miri, ignore)]
#[test]
fn visible_1600x900_detected() {
    let detector = RoundDetector::new(default_round_config());
    let raw = load_fixture("visible_1600x900.png");
    let game = crop_titlebar(&raw);
    assert!(
        is_visible(&detector.analyze(&game)),
        "expected RoundVisible for post-update 1600x900 frame"
    );
}
