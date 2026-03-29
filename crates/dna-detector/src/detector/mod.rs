//! Detection pipeline: analyze game frames and emit events.

pub mod ally_hp;
pub mod dialog;
pub mod round;
pub mod skill;

use image::RgbaImage;

use crate::event::DetectionEvent;

/// Trait for frame analyzers that produce detection events.
pub trait Detector {
    /// Analyze a single frame and return any detected events.
    fn analyze(&self, frame: &RgbaImage) -> Vec<DetectionEvent>;
}
