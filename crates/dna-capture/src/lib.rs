//! Windows screen capture and OCR backends for DNA Online assistant.
//!
//! This crate provides platform-specific capture implementations:
//! - Windows Graphics Capture API (WGC) — primary, GPU-accelerated
//! - `PrintWindow` API — fallback for environments where WGC is unavailable
//! - Windows OCR API — text recognition for round number extraction
//!
//! All capture methods return `image::RgbaImage` for consumption by `dna-detector`.
//!
//! # Platform support
//!
//! Windows API modules are gated behind `#[cfg(target_os = "windows")]`.
//! On Linux, this crate compiles as an empty library, allowing workspace-wide
//! `cargo check` / `cargo test` / `cargo clippy` in `DevContainer`.
//!
//! ```toml
//! [target.'cfg(target_os = "windows")'.dependencies]
//! windows-capture = "1.5"
//! win-screenshot = "4.0"
//! windows = { version = "0.62", features = ["Win32_UI_WindowsAndMessaging", "Win32_Foundation"] }
//! ```

use anyhow::Result;
use image::RgbaImage;

#[cfg(target_os = "windows")]
pub mod printwindow;
#[cfg(target_os = "windows")]
pub mod wgc;
#[cfg(target_os = "windows")]
pub mod window;

#[cfg(target_os = "windows")]
pub mod ocr;

/// Backend-agnostic screen capture interface.
///
/// Each backend (WGC, `PrintWindow`) implements this trait, enabling the
/// application layer to swap backends transparently.
pub trait Capture {
    /// Capture a single frame from the target window.
    ///
    /// Returns the full window content including the titlebar.
    /// Titlebar removal is handled by `dna-detector::titlebar::crop_titlebar()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the capture fails (e.g., window closed,
    /// API unavailable, or backend-specific failure).
    fn capture_frame(&mut self) -> Result<RgbaImage>;

    /// Check whether the target window still exists.
    fn is_window_alive(&self) -> bool;
}

/// Available capture backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureBackend {
    /// Windows Graphics Capture API (primary, GPU-accelerated).
    WindowsGraphicsCapture,
    /// `PrintWindow` API (fallback).
    PrintWindow,
}
