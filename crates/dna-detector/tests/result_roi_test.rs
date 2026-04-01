//! ROI extraction tests for result screen detection.
//!
//! Masks full-frame fixtures using config ROI, then crops and saves
//! ROI images for visual inspection.
//!
//! Run with: `cargo test -p dna-detector --test result_roi_test -- --ignored`

use dna_detector::config::ResultScreenRoiConfig;
use dna_detector::roi::{PixelRect, RoiDefinition};
use dna_detector::titlebar::crop_titlebar;

// ── ROI definition (single source of truth: config default) ─────────

const ROI: RoiDefinition = ResultScreenRoiConfig::const_default().text;

// ── Test helpers ────────────────────────────────────────────────────

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!(
        "{}/tests/fixtures/result/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Save an image to the fixtures directory.
#[allow(clippy::panic)]
fn save_fixture(name: &str, image: &image::RgbaImage) {
    let dir = format!("{}/tests/fixtures/result", env!("CARGO_MANIFEST_DIR"));
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

// ── Fixture mask + crop test ────────────────────────────────────────

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "generates ROI crops for visual inspection"]
fn crop_result_1602x932() {
    let raw = load_fixture("result_1602x932.png");

    let masked = mask_fixture(&raw, &[&ROI]);
    save_fixture("result_1602x932.png", &masked);

    let game = crop_titlebar(&masked);
    if let Some(crop) = ROI.crop(&game) {
        save_fixture("roi_1602x932.png", &crop);
    }
}
