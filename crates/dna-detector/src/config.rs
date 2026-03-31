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
    /// ROI for the round text area.
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

/// ROI definitions for round number OCR detection.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundNumberRoiConfig {
    /// ROI for the "XX ラウンド終了" screen (number + text).
    pub round_end: RoiDefinition,
    /// ROI for "自動周回中（X/5）" header text on the selection screen.
    pub select_header: RoiDefinition,
    /// ROI for the next round entry on the right panel.
    pub select_next_round: RoiDefinition,
    /// ROI for the latest completed round on the left panel.
    pub select_completed_round: RoiDefinition,
}

impl Default for RoundNumberRoiConfig {
    fn default() -> Self {
        Self {
            round_end: RoiDefinition {
                x: 0.35,
                y: 0.38,
                width: 0.30,
                height: 0.18,
            },
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

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            round: RoundDetectorConfig {
                roi: RoiDefinition {
                    x: 0.0,
                    y: 0.256,
                    width: 0.250,
                    height: 0.035,
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

        // Thresholds are positive
        assert!(config.round.text_presence_threshold > 0.0);
        assert!(config.dialog.text_presence_threshold > 0.0);
        assert!(config.dialog.bg_dark_threshold > 0.0);
    }

    #[test]
    fn round_number_roi_config_has_valid_ranges() {
        let config = RoundNumberRoiConfig::default();

        // round_end ROI
        assert!(config.round_end.x + config.round_end.width <= 1.0);
        assert!(config.round_end.y + config.round_end.height <= 1.0);

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
