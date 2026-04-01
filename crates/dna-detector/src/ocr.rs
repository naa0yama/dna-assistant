//! Platform-independent OCR abstraction.
//!
//! Defines the [`OcrEngine`] trait that detection logic uses to request
//! text recognition. The concrete implementation lives in `dna-capture`
//! (Windows OCR) and is injected at construction time.

use image::RgbaImage;

/// Platform-independent OCR engine trait.
///
/// Detectors accept `&dyn OcrEngine` so they can call OCR without
/// depending on platform-specific crates. The implementation handles
/// binarization and the actual recognition call.
#[allow(clippy::module_name_repetitions)]
pub trait OcrEngine {
    /// Recognise text from an RGBA image region.
    ///
    /// The implementation should binarize the image as needed and return
    /// all recognised text as a single string.
    ///
    /// # Errors
    ///
    /// Returns an error message string if recognition fails.
    fn recognize(&self, image: &RgbaImage) -> Result<String, String>;
}
