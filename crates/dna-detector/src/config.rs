//! Configuration types for detection parameters.

use serde::{Deserialize, Serialize};

use crate::color::HsvRange;
use crate::roi::RoiDefinition;

/// Top-level detection configuration.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectionConfig {
    /// Skill button detector settings.
    pub skill: SkillDetectorConfig,
    /// Ally HP detector settings.
    pub ally_hp: AllyHpDetectorConfig,
    /// Round detector settings.
    pub round: RoundDetectorConfig,
    /// Dialog detector settings.
    pub dialog: DialogDetectorConfig,
}

/// Configuration for the skill (Q) SP depletion detector.
///
/// Detects when the Q skill icon becomes greyed out due to SP exhaustion.
/// A greyed-out icon has very low maximum brightness and no bright icon pixels.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDetectorConfig {
    /// ROI for the Q skill icon area (excluding SP number and label text).
    pub roi: RoiDefinition,
    /// Maximum brightness below which the icon is considered greyed out.
    /// Normal icons have max brightness ~255; greyed-out icons ~100.
    pub greyed_max_brightness: u8,
    /// Minimum ratio of bright pixels (brightness > `icon_brightness_min`)
    /// for the icon to be considered active. Greyed-out icons have 0%.
    pub icon_bright_threshold: f64,
    /// Minimum brightness for a pixel to count as part of the visible icon.
    pub icon_brightness_min: u8,
}

/// Configuration for the ally HP detector.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllyHpDetectorConfig {
    /// ROI for the ally status area.
    pub roi: RoiDefinition,
    /// HSV range that matches HP bar color (green).
    pub hp_color_range: HsvRange,
    /// HP ratio below which an ally is considered "down" (e.g., 0.05).
    pub down_threshold: f64,
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
    /// Round text "探検" starts from the left with white characters (~255).
    /// Result screen backgrounds never reach this brightness in the left area (~168 max).
    pub text_left_brightness_min: u8,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            skill: SkillDetectorConfig {
                roi: RoiDefinition {
                    x: 0.878,
                    y: 0.880,
                    width: 0.042,
                    height: 0.038,
                },
                greyed_max_brightness: 140,
                icon_bright_threshold: 0.05,
                icon_brightness_min: 120,
            },
            ally_hp: AllyHpDetectorConfig {
                roi: RoiDefinition {
                    x: 0.01,
                    y: 0.78,
                    width: 0.12,
                    height: 0.15,
                },
                hp_color_range: HsvRange {
                    h_min: 80.0,
                    h_max: 150.0,
                    s_min: 0.3,
                    s_max: 1.0,
                    v_min: 0.3,
                    v_max: 1.0,
                },
                down_threshold: 0.05,
            },
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

        // Skill ROI is within bounds
        assert!(config.skill.roi.x + config.skill.roi.width <= 1.0);
        assert!(config.skill.roi.y + config.skill.roi.height <= 1.0);

        // Round ROI is within bounds
        assert!(config.round.roi.x + config.round.roi.width <= 1.0);
        assert!(config.round.roi.y + config.round.roi.height <= 1.0);

        // Ally HP ROI is within bounds
        assert!(config.ally_hp.roi.x + config.ally_hp.roi.width <= 1.0);
        assert!(config.ally_hp.roi.y + config.ally_hp.roi.height <= 1.0);

        // Dialog text ROI is within bounds
        assert!(config.dialog.text_roi.x + config.dialog.text_roi.width <= 1.0);
        assert!(config.dialog.text_roi.y + config.dialog.text_roi.height <= 1.0);

        // Dialog bg ROI is within bounds
        assert!(config.dialog.bg_roi.x + config.dialog.bg_roi.width <= 1.0);
        assert!(config.dialog.bg_roi.y + config.dialog.bg_roi.height <= 1.0);

        // Thresholds are positive
        assert!(config.skill.icon_bright_threshold > 0.0);
        assert!(config.round.text_presence_threshold > 0.0);
        assert!(config.ally_hp.down_threshold > 0.0);
        assert!(config.dialog.text_presence_threshold > 0.0);
        assert!(config.dialog.bg_dark_threshold > 0.0);
    }
}
