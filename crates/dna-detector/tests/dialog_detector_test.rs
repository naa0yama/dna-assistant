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
        ocr_roi: RoiDefinition {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
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

/// Full-frame test using default config (not cropped fixture).
/// The fixture includes the titlebar, so we apply `crop_titlebar` first.
#[cfg_attr(miri, ignore)]
#[test]
fn visible_nw_error_1368x800_full_frame() {
    use dna_detector::config::DetectionConfig;
    use dna_detector::titlebar::crop_titlebar;

    let raw = load_fixture("visible_nw_error_1368x800.png");
    let frame = crop_titlebar(&raw);
    let config = DetectionConfig::default();
    let detector = DialogDetector::new(config.dialog);
    let events = detector.analyze(&frame);
    assert!(
        is_visible(&events),
        "expected DialogVisible for 1368x800 full frame, got {events:?}"
    );
}

/// One-shot utility: mask the full-frame fixture to black outside the
/// dialog ROI bounding box, then re-save as optimized PNG.
/// Run manually: `cargo test -p dna-detector --test dialog_detector_test -- mask_fixture --ignored --nocapture`
/// One-shot utility: mask the full-frame fixture to black outside the
/// dialog ROI bounding box, then re-save as optimized PNG.
/// Run manually: `cargo test -p dna-detector --test dialog_detector_test -- mask_fixture --ignored --nocapture`
#[ignore = "one-shot fixture masking utility"]
#[test]
#[allow(
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions
)]
fn mask_fixture() {
    use dna_detector::config::DetectionConfig;
    use dna_detector::titlebar::crop_titlebar;
    use image::Rgba;

    let fixture = "visible_nw_error_1368x800.png";
    let path = format!(
        "{}/tests/fixtures/dialog/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = image::open(&path).unwrap().to_rgba8();
    let (w, h) = (raw.width(), raw.height());

    // Detect titlebar height
    let game = crop_titlebar(&raw);
    let tb = h - game.height();
    let gh = f64::from(game.height());
    let fw = f64::from(w);

    let cfg = DetectionConfig::default();
    // Keep union of bg_roi and text_roi with margin
    let margin = 0.06;
    let min_x = cfg.dialog.bg_roi.x.min(cfg.dialog.text_roi.x);
    let min_y = cfg.dialog.bg_roi.y.min(cfg.dialog.text_roi.y);
    let max_x = (cfg.dialog.bg_roi.x + cfg.dialog.bg_roi.width)
        .max(cfg.dialog.text_roi.x + cfg.dialog.text_roi.width);
    let max_y = (cfg.dialog.bg_roi.y + cfg.dialog.bg_roi.height)
        .max(cfg.dialog.text_roi.y + cfg.dialog.text_roi.height);

    let kx1 = ((min_x - margin).max(0.0) * fw) as u32;
    let ky1 = tb + ((min_y - margin).max(0.0) * gh) as u32;
    let kx2 = ((max_x + margin).min(1.0) * fw) as u32;
    let ky2 = tb + ((max_y + margin).min(1.0) * gh) as u32;

    let mut masked = raw;
    let black = Rgba([0u8, 0, 0, 255]);
    for y in 0..h {
        for x in 0..w {
            // Preserve titlebar so crop_titlebar() works on the masked image
            if y >= tb && (x < kx1 || x >= kx2 || y < ky1 || y >= ky2) {
                masked.put_pixel(x, y, black);
            }
        }
    }

    masked.save(&path).unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    eprintln!("Masked {fixture}: {w}x{h}, keep ({kx1},{ky1})-({kx2},{ky2}), {size} bytes");
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
