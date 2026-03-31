//! Integration test: run Windows OCR on round number fixture images.
//!
//! Verifies actual OCR output for `round_end` and `round_select` ROIs.
//! Windows-only (requires `dna-capture` OCR engine).
//!
//! Run with: `cargo test -p dna-assistant --test round_number_ocr_test -- --ignored --nocapture`

#![cfg(target_os = "windows")]

use dna_capture::ocr::{JapaneseOcrEngine, binarize_white_text};
use dna_detector::config::RoundNumberRoiConfig;
use dna_detector::round_number::{
    is_round_end_text, is_round_select_text, parse, parse_select_header,
};
use dna_detector::titlebar::crop_titlebar;

/// Load a fixture from dna-detector's test fixtures directory.
#[allow(clippy::panic)]
fn load_fixture(subdir: &str, name: &str) -> image::RgbaImage {
    // Navigate from src-tauri to crates/dna-detector
    let path = format!(
        "{}/../crates/dna-detector/tests/fixtures/{subdir}/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    image::open(&path)
        .unwrap_or_else(|e| panic!("failed to load fixture {path}: {e}"))
        .to_rgba8()
}

/// Run OCR on a cropped + binarized ROI and return the text.
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

// ── Round end screen OCR ────────────────────────────────────────────

const ROUND_END_FIXTURES: &[(&str, u32)] = &[
    ("round_end_1282x752.png", 8),
    ("round_end_1368x800.png", 4),
    ("round_end_1602x932.png", 13),
    ("round_end_1922x1112.png", 19),
    ("round_01_end.png", 1),
    ("round_03_end.png", 3),
];

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "Windows-only OCR integration test"]
fn ocr_round_end_all_resolutions() {
    let engine = JapaneseOcrEngine::new().expect("OCR engine init failed");
    let rois = RoundNumberRoiConfig::default();

    for (fixture, expected_round) in ROUND_END_FIXTURES {
        let raw = load_fixture("round_end", fixture);
        let game = crop_titlebar(&raw);

        let text = ocr_roi(&engine, &game, &rois.round_end);
        let is_end = is_round_end_text(&text);
        let round_num = parse(&text);

        eprintln!(
            "{fixture} ({}x{}): is_end={is_end} round={round_num:?} ocr=\"{text}\"",
            game.width(),
            game.height()
        );

        if !is_end || round_num != Some(*expected_round) {
            eprintln!("  ** MISMATCH: expected round={expected_round}");
        }
    }
}

// ── Round select screen OCR ─────────────────────────────────────────

const ROUND_SELECT_FIXTURES: &[&str] = &[
    "round_select_1282x752.png",
    "round_select_1368x800.png",
    "round_select_1602x932.png",
    "round_select_1922x1112.png",
    "round_select_after_01.png",
    "round_select_after_03.png",
];

#[cfg_attr(miri, ignore)]
#[test]
#[ignore = "Windows-only OCR integration test"]
fn ocr_round_select_all_resolutions() {
    let engine = JapaneseOcrEngine::new().expect("OCR engine init failed");
    let rois = RoundNumberRoiConfig::default();

    for fixture in ROUND_SELECT_FIXTURES {
        let raw = load_fixture("round_select", fixture);
        let game = crop_titlebar(&raw);

        let header_text = ocr_roi(&engine, &game, &rois.select_header);
        let is_select = is_round_select_text(&header_text);
        let header_round = parse_select_header(&header_text);

        let right_text = ocr_roi(&engine, &game, &rois.select_next_round);
        let right_round = parse(&right_text);

        let left_text = ocr_roi(&engine, &game, &rois.select_completed_round);
        let left_round = parse(&left_text);

        eprintln!("{fixture} ({}x{}):", game.width(), game.height());
        eprintln!(
            "  header: is_select={is_select} header_round={header_round:?} ocr=\"{header_text}\""
        );
        eprintln!("  right:  round={right_round:?} ocr=\"{right_text}\"");
        eprintln!("  left:   round={left_round:?} ocr=\"{left_text}\"");

        assert!(
            is_select,
            "{fixture}: expected is_select=true, ocr=\"{header_text}\""
        );
    }
}
