//! Detection events emitted by the detection pipeline.

use std::time::Instant;

use crate::round_number::StageKind;

/// Events produced by analyzing game frames.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub enum DetectionEvent {
    /// Round text is visible with a detected round number.
    RoundVisible {
        /// Whether round text presence was detected via pixel density.
        text_present: bool,
        /// White pixel density in the ROI.
        white_ratio: f64,
        /// OCR-recognized round number (None if OCR unavailable or failed).
        round_number: Option<u32>,
        /// Stage kind extracted from OCR text (None until OCR runs).
        stage_kind: Option<StageKind>,
        /// When this was detected.
        timestamp: Instant,
    },
    /// Round text has disappeared (possible stage completion).
    RoundGone {
        /// White pixel density in the ROI.
        white_ratio: f64,
        /// When this was detected.
        timestamp: Instant,
    },
    /// A dialog box (e.g., network error "Tips") is visible on screen.
    DialogVisible {
        /// Low-chroma text pixel ratio in the text ROI.
        text_ratio: f64,
        /// Dark pixel ratio in the background ROI.
        bg_dark_ratio: f64,
        /// When this was detected.
        timestamp: Instant,
    },
    /// No dialog box detected.
    DialogGone {
        /// Low-chroma text pixel ratio in the text ROI.
        text_ratio: f64,
        /// Dark pixel ratio in the background ROI.
        bg_dark_ratio: f64,
        /// When this was detected.
        timestamp: Instant,
    },
    /// Result screen detected via OCR ("依頼完了" text recognized).
    ResultScreenVisible {
        /// OCR-recognized text.
        text: String,
        /// When this was detected.
        timestamp: Instant,
    },
    /// Result screen no longer visible (OCR did not find "依頼完了").
    ResultScreenGone {
        /// When this was detected.
        timestamp: Instant,
    },
    /// Genemon liberation quest text is visible in the quest ROI.
    GenemonVisible {
        /// When this was detected.
        timestamp: Instant,
    },
    /// Genemon liberation quest text is no longer visible.
    GenemonGone {
        /// When this was detected.
        timestamp: Instant,
    },
    /// Round selection screen detected ("自動周回中").
    RoundSelectScreen {
        /// Next round number from right panel (1-99).
        next_round: Option<u32>,
        /// Latest completed round from left panel (1-99).
        completed_round: Option<u32>,
        /// When this was detected.
        timestamp: Instant,
    },
}
