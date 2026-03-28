//! Detection events emitted by the detection pipeline.

use std::time::Instant;

/// Events produced by analyzing game frames.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone)]
pub enum DetectionEvent {
    /// Q skill is available (icon visible, not greyed out).
    SkillReady {
        /// Ratio of bright icon pixels in the ROI.
        icon_bright_ratio: f64,
        /// Maximum brightness in the ROI.
        max_brightness: u8,
        /// When this was detected.
        timestamp: Instant,
    },
    /// Q skill icon is greyed out (SP depleted).
    SkillGreyed {
        /// Ratio of bright icon pixels in the ROI.
        icon_bright_ratio: f64,
        /// Maximum brightness in the ROI.
        max_brightness: u8,
        /// When this was detected.
        timestamp: Instant,
    },
    /// An ally's HP is critically low.
    AllyHpLow {
        /// Which ally (0-indexed).
        ally_index: u8,
        /// Remaining HP ratio (0.0..=1.0).
        hp_ratio: f64,
        /// When this was detected.
        timestamp: Instant,
    },
    /// An ally's HP has recovered above the critical threshold.
    AllyHpNormal {
        /// Which ally (0-indexed).
        ally_index: u8,
        /// Current HP ratio (0.0..=1.0).
        hp_ratio: f64,
        /// When this was detected.
        timestamp: Instant,
    },
    /// Round text is visible with a detected round number.
    RoundVisible {
        /// Whether round text presence was detected via pixel density.
        text_present: bool,
        /// White pixel density in the ROI.
        white_ratio: f64,
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
}
