//! Windows Graphics Capture API backend (primary).
//!
//! Uses the `windows-capture` crate to capture the game window via the WGC API.
//! GPU-accelerated and works with occluded (background) windows.
//! Requires Windows 10 1903+ (Build 18362).

use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, bail};
use image::RgbaImage;
use tracing::{debug, instrument, warn};
use windows::Win32::Foundation::HWND;
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

use crate::Capture;
use crate::window;

/// Shared state between the WGC callback handler and the [`Capturer`] owner.
#[derive(Debug, Default)]
struct SharedFrameState {
    /// Latest captured frame, replaced on each `on_frame_arrived` callback.
    latest_frame: Option<RgbaImage>,
}

/// Internal handler implementing [`GraphicsCaptureApiHandler`].
///
/// Stores captured frames into shared state accessible via
/// [`CaptureControl::callback()`].
struct Handler {
    state: Arc<Mutex<SharedFrameState>>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Arc<Mutex<SharedFrameState>>;
    type Error = anyhow::Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self> {
        debug!("WGC handler initialized");
        Ok(Self { state: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<()> {
        let width = frame.width();
        let height = frame.height();

        let mut buffer = frame
            .buffer()
            .context("failed to access WGC frame buffer")?;
        let pixels = buffer
            .as_nopadding_buffer()
            .context("failed to read WGC frame pixels")?
            .to_vec();

        let image = RgbaImage::from_raw(width, height, pixels)
            .context("failed to construct RgbaImage from WGC frame")?;

        let Ok(mut state) = self.state.lock() else {
            warn!("shared frame state lock poisoned");
            return Ok(());
        };
        state.latest_frame = Some(image);

        Ok(())
    }

    fn on_closed(&mut self) -> Result<()> {
        debug!("WGC capture session closed");
        Ok(())
    }
}

/// Screen capture backend using the Windows Graphics Capture API.
///
/// Launches a background capture thread that receives frame callbacks from WGC.
/// [`Capture::capture_frame`] returns the most recently received frame.
pub struct Capturer {
    hwnd: HWND,
    state: Arc<Mutex<SharedFrameState>>,
    #[allow(dead_code)] // kept alive to maintain the capture session
    control: CaptureControl<Handler, anyhow::Error>,
}

impl std::fmt::Debug for Capturer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capturer")
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

impl Capturer {
    /// Start a new WGC capture session for the given window.
    ///
    /// The yellow capture border is disabled via
    /// [`DrawBorderSettings::WithoutBorder`] (requires Windows 10 Build 20348+).
    ///
    /// # Errors
    ///
    /// Returns an error if the WGC session cannot be started (e.g., window not
    /// found, unsupported OS version, or API failure).
    #[instrument]
    pub fn start(hwnd: HWND) -> Result<Self> {
        let window = Window::from_raw_hwnd(hwnd.0);

        let state = Arc::new(Mutex::new(SharedFrameState::default()));

        let settings = Settings::new(
            window,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            state.clone(),
        );

        let control = Handler::start_free_threaded(settings)
            .context("failed to start WGC capture session")?;

        debug!(?hwnd, "WGC capture session started");

        Ok(Self {
            hwnd,
            state,
            control,
        })
    }
}

impl Capture for Capturer {
    #[instrument(skip(self))]
    fn capture_frame(&mut self) -> Result<RgbaImage> {
        let state = self
            .state
            .lock()
            .map_err(|e| anyhow::anyhow!("shared frame state lock poisoned: {e}"))?;

        match state.latest_frame.clone() {
            Some(frame) => Ok(frame),
            None => bail!("no frame available yet from WGC"),
        }
    }

    fn is_window_alive(&self) -> bool {
        window::is_window_alive(self.hwnd)
    }
}
