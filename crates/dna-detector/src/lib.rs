//! Cross-platform detection logic for DNA Online assistant.
//!
//! Analyzes captured game frames to detect state changes:
//! round completion, dialog detection, and round number extraction.
//! All logic is platform-independent — only requires `image::RgbaImage` as input.

pub mod color;
pub mod config;
pub mod detector;
pub mod event;
pub mod roi;
pub mod round_number;
pub mod state;
pub mod titlebar;
