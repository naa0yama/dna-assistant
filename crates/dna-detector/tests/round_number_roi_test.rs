//! ROI extraction tests for round number detection.
//!
//! These tests crop ROI regions from full-frame fixtures and save them
//! as PNG files for visual inspection. Full-frame fixtures are masked
//! (ROI-outside areas blacked out) to reduce file size while preserving
//! the titlebar and ROI regions for pipeline verification.
//!
//! Run with: `cargo test -p dna-detector --test round_number_roi_test -- --ignored`

use dna_detector::roi::{PixelRect, RoiDefinition};
use dna_detector::titlebar::crop_titlebar;

// ── Test helpers ────────────────────────────────────────────────────

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(subdir: &str, name: &str) -> image::RgbaImage {
    let path = format!(
        "{}/tests/fixtures/{subdir}/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Save an image to the fixtures directory.
#[allow(clippy::panic)]
fn save_fixture(subdir: &str, name: &str, image: &image::RgbaImage) {
    let dir = format!("{}/tests/fixtures/{subdir}", env!("CARGO_MANIFEST_DIR"));
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
///
/// Preserves the titlebar rows intact (needed for `crop_titlebar` tests),
/// and keeps ROI content for detection verification.
/// Everything else becomes black (0,0,0) for maximum PNG compression.
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

/// Run `round_end` ROI extraction for a given fixture.
#[allow(clippy::print_stderr)]
fn run_round_end_test(fixture_name: &str, roi_prefix: &str) {
    let raw = load_fixture("round_end", fixture_name);

    let masked = mask_fixture(&raw, &[&ROUND_END_NUMBER_ROI]);
    save_fixture("round_end", fixture_name, &masked);

    let game = crop_titlebar(&masked);
    if let Some(roi) = ROUND_END_NUMBER_ROI.crop(&game) {
        save_fixture("round_end", &format!("roi_{roi_prefix}_number.png"), &roi);
    }
}

/// Run `round_select` ROI extraction for a given fixture.
fn run_round_select_test(fixture_name: &str, roi_prefix: &str) {
    let raw = load_fixture("round_select", fixture_name);

    let masked = mask_fixture(&raw, ROUND_SELECT_ROIS);
    save_fixture("round_select", fixture_name, &masked);

    let game = crop_titlebar(&masked);

    if let Some(roi) = ROUND_SELECT_HEADER_ROI.crop(&game) {
        save_fixture(
            "round_select",
            &format!("roi_{roi_prefix}_header.png"),
            &roi,
        );
    }
    if let Some(roi) = ROUND_SELECT_LEFT_TOP_ROI.crop(&game) {
        save_fixture(
            "round_select",
            &format!("roi_{roi_prefix}_left_top.png"),
            &roi,
        );
    }
    if let Some(roi) = ROUND_SELECT_RIGHT_TOP_ROI.crop(&game) {
        save_fixture(
            "round_select",
            &format!("roi_{roi_prefix}_right_top.png"),
            &roi,
        );
    }
}

// ── ROI definitions ─────────────────────────────────────────────────

/// ROI for the round end screen: "XX" + "ラウンド終了".
const ROUND_END_NUMBER_ROI: RoiDefinition = RoiDefinition {
    x: 0.40,
    y: 0.43,
    width: 0.20,
    height: 0.11,
};

/// ROI for "自動周回中（X/Y）" header text on the selection screen.
const ROUND_SELECT_HEADER_ROI: RoiDefinition = RoiDefinition {
    x: 0.20,
    y: 0.10,
    width: 0.60,
    height: 0.04,
};

/// ROI for the latest completed round at the top of the left panel.
const ROUND_SELECT_LEFT_TOP_ROI: RoiDefinition = RoiDefinition {
    x: 0.195,
    y: 0.33,
    width: 0.11,
    height: 0.10,
};

/// ROI for the next round on the right panel.
const ROUND_SELECT_RIGHT_TOP_ROI: RoiDefinition = RoiDefinition {
    x: 0.52,
    y: 0.37,
    width: 0.11,
    height: 0.07,
};

const ROUND_SELECT_ROIS: &[&RoiDefinition] = &[
    &ROUND_SELECT_HEADER_ROI,
    &ROUND_SELECT_LEFT_TOP_ROI,
    &ROUND_SELECT_RIGHT_TOP_ROI,
];

// ── Round end tests (all resolutions) ───────────────────────────────

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_end_1282x752() {
    run_round_end_test("round_end_1282x752.png", "1282x752");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_end_1368x800() {
    run_round_end_test("round_end_1368x800.png", "1368x800");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_end_1602x932() {
    run_round_end_test("round_end_1602x932.png", "1602x932");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_end_1922x1112() {
    run_round_end_test("round_end_1922x1112.png", "1922x1112");
}

// Legacy fixtures from initial video analysis
#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_end_01() {
    run_round_end_test("round_01_end.png", "01");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_end_03() {
    run_round_end_test("round_03_end.png", "03");
}

// ── Round select tests (all resolutions) ────────────────────────────

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_select_1282x752() {
    run_round_select_test("round_select_1282x752.png", "1282x752");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_select_1368x800() {
    run_round_select_test("round_select_1368x800.png", "1368x800");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_select_1602x932() {
    run_round_select_test("round_select_1602x932.png", "1602x932");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_select_1922x1112() {
    run_round_select_test("round_select_1922x1112.png", "1922x1112");
}

// Legacy fixtures
#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_select_after_01() {
    run_round_select_test("round_select_after_01.png", "after_01");
}

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_round_select_after_03() {
    run_round_select_test("round_select_after_03.png", "after_03");
}
