//! Windows OCR API wrapper for Japanese text recognition.
//!
//! Uses `Windows.Media.Ocr` to recognize Japanese text from game frame ROIs.
//! The OCR engine is initialized once and reused across frames.

use anyhow::{Context as _, Result, bail};
use image::RgbaImage;
use tracing::{debug, instrument};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

/// Windows OCR engine wrapper, pre-initialized with Japanese language.
///
/// Create once at monitor startup, reuse across frames.
#[derive(Debug)]
pub struct JapaneseOcrEngine {
    engine: OcrEngine,
}

impl JapaneseOcrEngine {
    /// Create a new OCR engine for Japanese text recognition.
    ///
    /// # Errors
    ///
    /// Returns an error if the Japanese language pack is not installed
    /// or the OCR engine cannot be created.
    pub fn new() -> Result<Self> {
        let language =
            Language::CreateLanguage(&HSTRING::from("ja")).context("failed to create Language")?;

        if !OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
            bail!(
                "Japanese language pack not installed. \
                 Install it via Windows Settings > Time & Language > Language."
            );
        }

        let engine = OcrEngine::TryCreateFromLanguage(&language)
            .context("failed to create OCR engine for Japanese")?;

        debug!("Japanese OCR engine initialized");
        Ok(Self { engine })
    }

    /// Run OCR on a cropped RGBA image region.
    ///
    /// Returns all recognized text as a single string.
    ///
    /// # Errors
    ///
    /// Returns an error if bitmap conversion or OCR recognition fails.
    #[instrument(skip_all)]
    pub fn recognize_text(&self, image: &RgbaImage) -> Result<String> {
        let bitmap =
            rgba_to_software_bitmap(image).context("failed to convert to SoftwareBitmap")?;

        let result = self
            .engine
            .RecognizeAsync(&bitmap)
            .context("RecognizeAsync failed")?
            .join()
            .context("OCR recognition failed")?;

        let text = result.Text().context("failed to get OCR text")?;
        Ok(text.to_string_lossy())
    }
}

/// Convert an `RgbaImage` to a Windows `SoftwareBitmap` (Bgra8 format).
///
/// Windows OCR only supports `Bgra8` and `Gray8` pixel formats. RGBA pixels
/// are converted to BGRA by swapping the R and B channels.
fn rgba_to_software_bitmap(image: &RgbaImage) -> Result<SoftwareBitmap> {
    let width = i32::try_from(image.width()).context("width overflow")?;
    let height = i32::try_from(image.height()).context("height overflow")?;

    // Convert RGBA to BGRA (swap R and B channels)
    let bgra = rgba_to_bgra(image.as_raw());

    // Write BGRA pixels into an IBuffer via DataWriter
    let writer = DataWriter::new().context("failed to create DataWriter")?;
    writer.WriteBytes(&bgra).context("failed to write pixels")?;
    let buffer = writer.DetachBuffer().context("failed to detach buffer")?;

    SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width, height)
        .context("failed to create SoftwareBitmap from buffer")
}

/// Convert RGBA pixel buffer to BGRA by swapping R and B channels.
fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = rgba.to_vec();
    for chunk in bgra.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    bgra
}
