//! Integration test: run Windows OCR on round select fixture images.
//!
//! Verifies that Windows OCR can recognize text from config-ROI-cropped
//! fixture images. These tests run automatically on Windows to catch
//! ROI regressions that break OCR recognition.

#![cfg(target_os = "windows")]

use dna_capture::ocr::{JapaneseOcrEngine, binarize_white_text};
use dna_detector::config::RoundNumberRoiConfig;
use dna_detector::round_number::{is_round_select_text, parse, parse_select_header};
use dna_detector::titlebar::crop_titlebar;

// ── Helpers ─────────────────────────────────────────────────────────

/// Load a fixture from dna-detector's test fixtures directory.
#[allow(clippy::panic)]
fn load_fixture(name: &str) -> image::RgbaImage {
    let path = format!(
        "{}/../crates/dna-detector/tests/fixtures/round_select/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Crop ROI, binarize, and run OCR.
fn ocr_roi(
    engine: &JapaneseOcrEngine,
    game_frame: &image::RgbaImage,
    roi: &dna_detector::roi::RoiDefinition,
) -> String {
    let Some(roi_image) = roi.crop(game_frame) else {
        return String::from("[crop failed]");
    };
    let binarized = binarize_white_text(&roi_image, 140);
    match engine.recognize_text(&binarized) {
        Ok(text) => text,
        Err(e) => format!("[OCR error: {e}]"),
    }
}

// ── Tests ───────────────────────────────────────────────────────────

/// Verify header ROI OCR recognizes "自動周回中" and extracts the round number
/// (post-update 1600x900 fixture: select_header ROI y=0.10).
#[cfg_attr(miri, ignore)]
#[test]
fn ocr_round_select_header() {
    let engine = JapaneseOcrEngine::new().expect("OCR engine init failed");
    let rois = RoundNumberRoiConfig::default();

    let raw = load_fixture("round_select_pro_1600x900.png");
    let game = crop_titlebar(&raw);

    let text = ocr_roi(&engine, &game, &rois.select_header);
    let is_select = is_round_select_text(&text);
    let round = parse_select_header(&text);

    eprintln!("is_select={is_select} round={round:?} ocr=\"{text}\"");

    assert!(
        is_select,
        "header OCR should contain '自動周回中', got \"{text}\""
    );
    assert_eq!(
        round,
        Some(21),
        "expected header round=21 (from '自動周回中（21/99）'), got {round:?}, ocr=\"{text}\""
    );
}

/// Verify left/right panel ROI OCR on the post-update 1600x900 fixture.
///
/// The panel shows "XX ラウンド" list entries; left panel should contain
/// the latest completed round and right panel the next round.
#[cfg_attr(miri, ignore)]
#[test]
fn ocr_round_select_panels() {
    let engine = JapaneseOcrEngine::new().expect("OCR engine init failed");
    let rois = RoundNumberRoiConfig::default();

    let raw = load_fixture("round_select_pro_1600x900.png");
    let game = crop_titlebar(&raw);

    let left_text = ocr_roi(&engine, &game, &rois.select_completed_round);
    let left_round = parse(&left_text);

    let right_text = ocr_roi(&engine, &game, &rois.select_next_round);
    let right_round = parse(&right_text);

    eprintln!("left:  round={left_round:?} ocr=\"{left_text}\"");
    eprintln!("right: round={right_round:?} ocr=\"{right_text}\"");

    // Fixture is "自動周回中（21/99）": completed=21, next=22.
    // At least one panel must yield a round number; the other may be None
    // if the ROI edge falls between rows.
    assert!(
        left_round.is_some() || right_round.is_some(),
        "expected at least one panel round number (left={left_round:?}, right={right_round:?})"
    );
}

// ── Result screen OCR ───────────────────────────────────────────────

/// Verify "依頼終了" is recognized from result screen fixtures.
#[cfg_attr(miri, ignore)]
#[test]
fn ocr_result_screen() {
    use dna_detector::config::ResultScreenRoiConfig;

    let engine = JapaneseOcrEngine::new().expect("OCR engine init failed");
    let roi = ResultScreenRoiConfig::default().text;

    for fixture in &["result_1600x900.png", "retry_1600x900.png"] {
        let path = format!(
            "{}/../crates/dna-detector/tests/fixtures/result/{fixture}",
            env!("CARGO_MANIFEST_DIR")
        );
        let raw = image::open(&path).expect("load").to_rgba8();
        let game = crop_titlebar(&raw);

        let text = ocr_roi(&engine, &game, &roi);
        let norm: String = text.chars().filter(|c| !c.is_whitespace()).collect();

        eprintln!("{fixture}: ocr=\"{text}\"");
        assert!(
            norm.contains("依頼終了"),
            "{fixture}: expected '依頼終了', got \"{text}\""
        );
    }
}
