//! Integration tests for `GenemonDetector` using real game frame fixtures.
//!
//! OCR is stubbed with `StubOcr` — the fixture tests verify that the
//! detector correctly routes through the ROI crop path and returns the
//! expected event type for each keyword scenario.
//!
//! For a full end-to-end OCR test against real frames, see the
//! `mask_and_save_fixtures` ignored utility below.

use dna_detector::config::DetectionConfig;
use dna_detector::detector::genemon::GenemonDetector;
use dna_detector::event::DetectionEvent;
use dna_detector::ocr::OcrEngine;
use dna_detector::titlebar::crop_titlebar;

/// Stub OCR that returns a fixed string.
struct StubOcr(String);

impl OcrEngine for StubOcr {
    fn recognize(&self, _image: &image::RgbaImage) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

/// Load a fixture PNG as an `RgbaImage`.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!(
        "{}/tests/fixtures/genemon/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

const fn is_visible(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::GenemonVisible { .. }))
}

const fn is_gone(events: &[DetectionEvent]) -> bool {
    matches!(events.first(), Some(DetectionEvent::GenemonGone { .. }))
}

/// OCR returns "ジェネモンを探して解放しよう" → `GenemonVisible`.
#[cfg_attr(miri, ignore)]
#[test]
fn visible_genemon_quest() {
    let raw = load_fixture("visible.png");
    let frame = crop_titlebar(&raw);
    let config = DetectionConfig::default();
    let detector = GenemonDetector::new(config.genemon);
    let ocr = StubOcr("ジェネモンを探して解放しよう".into());
    let events = detector.analyze(&frame, &ocr);
    assert!(
        is_visible(&events),
        "expected GenemonVisible, got {events:?}"
    );
}

/// OCR returns unrelated text → `GenemonGone`.
#[cfg_attr(miri, ignore)]
#[test]
fn gone_no_genemon_quest() {
    let raw = load_fixture("gone.png");
    let frame = crop_titlebar(&raw);
    let config = DetectionConfig::default();
    let detector = GenemonDetector::new(config.genemon);
    let ocr = StubOcr("クエスト受注中".into());
    let events = detector.analyze(&frame, &ocr);
    assert!(is_gone(&events), "expected GenemonGone, got {events:?}");
}

/// One-shot utility: mask ROI area and re-save compressed fixtures.
/// Run manually:
///   cargo test -p dna-detector --test `genemon_detector_test` \
///     -- `mask_and_save_fixtures` --ignored --nocapture
#[ignore = "one-shot fixture masking utility"]
#[test]
#[allow(
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions
)]
fn mask_and_save_fixtures() {
    use dna_detector::config::DetectionConfig;
    use image::Rgba;

    let config = DetectionConfig::default();
    let roi = config.genemon.quest_roi;
    let margin = 0.02;

    for name in &["visible.png", "gone.png"] {
        let path = format!(
            "{}/tests/fixtures/genemon/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let raw = image::open(&path).unwrap().to_rgba8();
        let (w, h) = (raw.width(), raw.height());
        let game = crop_titlebar(&raw);
        let tb = h - game.height();

        let gw = f64::from(w);
        let gh = f64::from(game.height());
        let x1 = ((roi.x - margin).max(0.0) * gw) as u32;
        let y1 = tb + ((roi.y - margin).max(0.0) * gh) as u32;
        let x2 = ((roi.x + roi.width + margin).min(1.0) * gw) as u32;
        let y2 = tb + ((roi.y + roi.height + margin).min(1.0) * gh) as u32;

        let mut masked = raw;
        let black = Rgba([0u8, 0, 0, 255]);
        for y in 0..h {
            for x in 0..w {
                if y < tb || x < x1 || x >= x2 || y < y1 || y >= y2 {
                    masked.put_pixel(x, y, black);
                }
            }
        }
        masked.save(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        eprintln!("Masked {name}: {w}x{h}, keep ({x1},{y1})-({x2},{y2}), {size} bytes");
    }
}
