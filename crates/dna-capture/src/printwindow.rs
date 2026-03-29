//! `PrintWindow` API capture backend (fallback).
//!
//! Uses the `win-screenshot` crate to capture the game window via the Win32
//! `PrintWindow` API with `PW_RENDERFULLCONTENT`. This backend works with
//! occluded windows but may return black frames for DirectX games.

use anyhow::{Context as _, Result};
use image::RgbaImage;
use tracing::instrument;
use win_screenshot::capture::{Area, Using, capture_window_ex};
use windows::Win32::Foundation::HWND;

use crate::Capture;
use crate::window;

/// Screen capture backend using the Win32 `PrintWindow` API.
#[derive(Debug)]
pub struct Capturer {
    hwnd: HWND,
}

impl Capturer {
    /// Create a new `PrintWindow` capture backend for the given window.
    #[must_use]
    pub const fn new(hwnd: HWND) -> Self {
        Self { hwnd }
    }
}

impl Capture for Capturer {
    #[instrument(skip(self))]
    fn capture_frame(&mut self) -> Result<RgbaImage> {
        #[allow(clippy::as_conversions)] // HWND.0 is *mut c_void, isize needed by win-screenshot
        let hwnd_isize = self.hwnd.0 as isize;

        let buf = capture_window_ex(hwnd_isize, Using::PrintWindow, Area::Full, None, None)
            .context("PrintWindow capture failed")?;

        let rgba_pixels = bgra_to_rgba(&buf.pixels);

        RgbaImage::from_raw(buf.width, buf.height, rgba_pixels)
            .context("failed to construct RgbaImage from PrintWindow buffer")
    }

    fn is_window_alive(&self) -> bool {
        window::is_window_alive(self.hwnd)
    }
}

/// Convert a BGRA pixel buffer to RGBA by swapping the B and R channels.
fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = bgra.to_vec();
    for chunk in rgba.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    rgba
}
