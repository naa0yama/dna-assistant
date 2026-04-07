//! ROI extraction tests for round select screen detection.
//!
//! Masks full-frame fixtures using config ROIs, then crops and saves
//! ROI images for visual inspection.
//!
//! Run with: `cargo test -p dna-detector --test round_number_roi_test -- --ignored`

use dna_detector::config::RoundNumberRoiConfig;
use dna_detector::roi::{PixelRect, RoiDefinition};
use dna_detector::titlebar::crop_titlebar;

// ── ROI definitions (single source of truth: config defaults) ───────

const CONFIG: RoundNumberRoiConfig = RoundNumberRoiConfig::const_default();

const ROUND_SELECT_ROIS: &[(&str, &RoiDefinition)] = &[
    ("header", &CONFIG.select_header),
    ("left_top", &CONFIG.select_completed_round),
    ("right_top", &CONFIG.select_next_round),
];

// ── Test helpers ────────────────────────────────────────────────────

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!(
        "{}/tests/fixtures/round_select/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Save an image to the fixtures directory.
#[allow(clippy::panic)]
fn save_fixture(name: &str, image: &image::RgbaImage) {
    let dir = format!("{}/tests/fixtures/round_select", env!("CARGO_MANIFEST_DIR"));
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

/// Mask fixture and save ROI crops for a given resolution.
fn run_round_select_test(fixture_name: &str, roi_prefix: &str) {
    let raw = load_fixture(fixture_name);

    let roi_refs: Vec<&RoiDefinition> = ROUND_SELECT_ROIS.iter().map(|(_, r)| *r).collect();
    let masked = mask_fixture(&raw, &roi_refs);
    save_fixture(fixture_name, &masked);

    let game = crop_titlebar(&masked);
    for (name, roi) in ROUND_SELECT_ROIS {
        if let Some(crop) = roi.crop(&game) {
            save_fixture(&format!("roi_{roi_prefix}_{name}.png"), &crop);
        }
    }
}

// ── Fixture mask + crop tests ───────────────────────────────────────

/// Generate ROI crops from the post-update 1600x900 fixture.
#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_select_pro_1600x900() {
    run_round_select_test("round_select_pro_1600x900.png", "pro_1600x900");
}
