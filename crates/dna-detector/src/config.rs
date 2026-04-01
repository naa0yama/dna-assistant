//! Configuration types for detection parameters.

use serde::{Deserialize, Serialize};

use crate::roi::RoiDefinition;

/// Top-level detection configuration.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectionConfig {
    /// Round detector settings.
    pub round: RoundDetectorConfig,
    /// Dialog detector settings.
    pub dialog: DialogDetectorConfig,
}

/// Configuration for the dialog detector.
///
/// Detects centered dialog boxes (e.g., "Tips" network error) by combining
/// two criteria: high-density low-chroma text in a text ROI and a dark
/// background in a surrounding background ROI.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogDetectorConfig {
    /// ROI for the error message text band (center of dialog).
    pub text_roi: RoiDefinition,
    /// ROI for the dialog dark background area.
    pub bg_roi: RoiDefinition,
    /// ROI for OCR confirmation of dialog title ("Tips").
    ///
    /// Covers the center area where the dialog title appears.
    /// Used to gate pixel-detected dialogs via OCR text check.
    pub ocr_roi: RoiDefinition,
    /// Minimum low-chroma text pixel ratio to detect dialog (e.g., 0.10).
    pub text_presence_threshold: f64,
    /// Minimum dark pixel ratio in background ROI (e.g., 0.85).
    pub bg_dark_threshold: f64,
    /// Minimum average brightness for a pixel to be a text candidate.
    pub brightness_min: u8,
    /// Maximum chroma (max(R,G,B) - min(R,G,B)) for text pixels.
    pub max_chroma: u8,
    /// Maximum average brightness for a background pixel to count as dark.
    pub bg_brightness_max: u8,
}

/// Configuration for the round completion detector.
///
/// Detects the "探検 現在のラウンド：XX" text by measuring
/// high-brightness, low-chroma pixels (text-like) in the ROI.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundDetectorConfig {
    /// ROI for the round text area (pixel detection and OCR).
    pub roi: RoiDefinition,
    /// Minimum text pixel ratio to consider text present (e.g., 0.03).
    pub text_presence_threshold: f64,
    /// Minimum average brightness (0-255) for a pixel to be a text candidate.
    pub brightness_min: u8,
    /// Maximum chroma (max(R,G,B) - min(R,G,B)) to filter out colorful combat effects.
    pub max_chroma: u8,
    /// Minimum max brightness in the left quarter of the ROI to confirm text presence.
    pub text_left_brightness_min: u8,
}

/// ROI definitions for round selection screen OCR detection.
///
/// Detects round numbers from the "自動周回中" selection screen
/// which appears for 3-5 seconds after each round.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundNumberRoiConfig {
    /// ROI for "自動周回中（X/Y）" header text on the selection screen.
    pub select_header: RoiDefinition,
    /// ROI for the next round entry on the right panel.
    pub select_next_round: RoiDefinition,
    /// ROI for the latest completed round on the left panel.
    pub select_completed_round: RoiDefinition,
}

impl RoundNumberRoiConfig {
    /// Compile-time default for use in `const` contexts (e.g., test fixtures).
    #[must_use]
    pub const fn const_default() -> Self {
        Self {
            select_header: RoiDefinition {
                x: 0.20,
                y: 0.06,
                width: 0.60,
                height: 0.08,
            },
            select_next_round: RoiDefinition {
                x: 0.50,
                y: 0.32,
                width: 0.15,
                height: 0.12,
            },
            select_completed_round: RoiDefinition {
                x: 0.17,
                y: 0.28,
                width: 0.15,
                height: 0.15,
            },
        }
    }
}

impl Default for RoundNumberRoiConfig {
    fn default() -> Self {
        Self::const_default()
    }
}

/// ROI definition for result screen OCR detection.
///
/// Targets the "依頼終了" button text in the bottom-right footer bar.
/// This text is white on dark background, consistent across all mission
/// types and result screen variants (completion / retry).
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultScreenRoiConfig {
    /// ROI for the "依頼終了" text area (bottom-right footer).
    pub text: RoiDefinition,
}

impl ResultScreenRoiConfig {
    /// Compile-time default for use in `const` contexts (e.g., test fixtures).
    #[must_use]
    pub const fn const_default() -> Self {
        Self {
            text: RoiDefinition {
                x: 0.85,
                y: 0.93,
                width: 0.15,
                height: 0.07,
            },
        }
    }
}

impl Default for ResultScreenRoiConfig {
    fn default() -> Self {
        Self::const_default()
    }
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            round: RoundDetectorConfig {
                roi: RoiDefinition {
                    x: 0.0,
                    y: 0.25,
                    width: 0.237,
                    height: 0.10,
                },
                text_presence_threshold: 0.03,
                brightness_min: 140,
                max_chroma: 60,
                text_left_brightness_min: 200,
            },
            dialog: DialogDetectorConfig {
                text_roi: RoiDefinition {
                    x: 0.31,
                    y: 0.45,
                    width: 0.37,
                    height: 0.03,
                },
                bg_roi: RoiDefinition {
                    x: 0.25,
                    y: 0.40,
                    width: 0.50,
                    height: 0.15,
                },
                ocr_roi: RoiDefinition {
                    x: 0.25,
                    y: 0.35,
                    width: 0.50,
                    height: 0.20,
                },
                text_presence_threshold: 0.05,
                bg_dark_threshold: 0.70,
                brightness_min: 80,
                max_chroma: 60,
                bg_brightness_max: 50,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_valid_roi_ranges() {
        let config = DetectionConfig::default();

        // Round ROI is within bounds
        assert!(config.round.roi.x + config.round.roi.width <= 1.0);
        assert!(config.round.roi.y + config.round.roi.height <= 1.0);

        // Dialog text ROI is within bounds
        assert!(config.dialog.text_roi.x + config.dialog.text_roi.width <= 1.0);
        assert!(config.dialog.text_roi.y + config.dialog.text_roi.height <= 1.0);

        // Dialog bg ROI is within bounds
        assert!(config.dialog.bg_roi.x + config.dialog.bg_roi.width <= 1.0);
        assert!(config.dialog.bg_roi.y + config.dialog.bg_roi.height <= 1.0);

        // Dialog OCR ROI is within bounds
        assert!(config.dialog.ocr_roi.x + config.dialog.ocr_roi.width <= 1.0);
        assert!(config.dialog.ocr_roi.y + config.dialog.ocr_roi.height <= 1.0);

        // Thresholds are positive
        assert!(config.round.text_presence_threshold > 0.0);
        assert!(config.dialog.text_presence_threshold > 0.0);
        assert!(config.dialog.bg_dark_threshold > 0.0);
    }

    #[test]
    fn round_number_roi_config_has_valid_ranges() {
        let config = RoundNumberRoiConfig::default();

        // select_header ROI
        assert!(config.select_header.x + config.select_header.width <= 1.0);
        assert!(config.select_header.y + config.select_header.height <= 1.0);

        // select_next_round ROI
        assert!(config.select_next_round.x + config.select_next_round.width <= 1.0);
        assert!(config.select_next_round.y + config.select_next_round.height <= 1.0);

        // select_completed_round ROI
        assert!(config.select_completed_round.x + config.select_completed_round.width <= 1.0);
        assert!(config.select_completed_round.y + config.select_completed_round.height <= 1.0);
    }

    #[test]
    fn result_screen_roi_config_has_valid_ranges() {
        let config = ResultScreenRoiConfig::default();

        assert!(config.text.x + config.text.width <= 1.0);
        assert!(config.text.y + config.text.height <= 1.0);
    }

    #[test]
    fn result_screen_roi_config_serialization_roundtrip() {
        let config = ResultScreenRoiConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: ResultScreenRoiConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn round_number_roi_config_serialization_roundtrip() {
        let config = RoundNumberRoiConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: RoundNumberRoiConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn detection_config_serialization_roundtrip() {
        let config = DetectionConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: DetectionConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config, deserialized);
    }
}
