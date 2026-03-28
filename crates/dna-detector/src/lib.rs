//! Cross-platform detection logic for DNA Online assistant.
//!
//! Analyzes captured game frames to detect state changes:
//! round completion, skill activation, and ally HP status.
//! All logic is platform-independent — only requires `image::RgbaImage` as input.

pub mod color;
pub mod config;
pub mod detector;
pub mod event;
pub mod roi;
pub mod state;
pub mod titlebar;
