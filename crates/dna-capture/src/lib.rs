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
//! # Phase 2: conditional dependencies in Cargo.toml
//! [target.'cfg(target_os = "windows")'.dependencies]
//! windows-capture = "1"
//! windows = { version = "0.61", features = [...] }
//! ```

// Phase 2: uncomment when Windows capture is implemented
// #[cfg(target_os = "windows")]
// pub mod wgc;
// #[cfg(target_os = "windows")]
// pub mod printwindow;
// #[cfg(target_os = "windows")]
// pub mod window;
// #[cfg(target_os = "windows")]
// pub mod ocr;
