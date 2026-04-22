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

/// Shared state and callbacks passed from [`Capturer`] into the WGC handler thread.
struct HandlerContext {
    /// Latest captured frame, replaced on each `on_frame_arrived` callback.
    latest_frame: Mutex<Option<RgbaImage>>,
    /// Called on every frame arrival; used for `wgc.frames_received` metrics.
    on_frame: Box<dyn Fn() + Send + Sync + 'static>,
}

impl std::fmt::Debug for HandlerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerContext").finish_non_exhaustive()
    }
}

/// Internal handler implementing [`GraphicsCaptureApiHandler`].
///
/// Receives WGC frame callbacks and stores frames into shared state.
struct Handler {
    ctx: Arc<HandlerContext>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Arc<HandlerContext>;
    type Error = anyhow::Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self> {
        debug!("WGC handler initialized");
        Ok(Self {
            ctx: ctx.flags,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<()> {
        let width = frame.width();
        let height = frame.height();

        let buffer = frame
            .buffer()
            .context("failed to access WGC frame buffer")?;
        let pixels = buffer.as_nopadding_buffer(&mut self.scratch).to_vec();

        let image = RgbaImage::from_raw(width, height, pixels)
            .context("failed to construct RgbaImage from WGC frame")?;

        let Ok(mut latest) = self.ctx.latest_frame.lock() else {
            warn!("shared frame state lock poisoned");
            return Ok(());
        };
        *latest = Some(image);
        (self.ctx.on_frame)();

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
///
/// On drop, the `on_drop` callback is invoked and `CaptureControl::stop` is
/// called explicitly to join the capture thread.
pub struct Capturer {
    hwnd: HWND,
    ctx: Arc<HandlerContext>,
    /// Wrapped in `Option` so `Drop` can take ownership for `CaptureControl::stop`.
    control: Option<CaptureControl<Handler, anyhow::Error>>,
    /// Called once when this `Capturer` is dropped; used for metrics.
    on_drop: Box<dyn Fn() + Send + 'static>,
}

impl std::fmt::Debug for Capturer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capturer")
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

impl Drop for Capturer {
    fn drop(&mut self) {
        (self.on_drop)();
        if let Some(control) = self.control.take() {
            if let Err(e) = control.stop() {
                warn!(?e, "WGC capture session stop error on drop");
            }
        }
    }
}

impl Capturer {
    /// Start a new WGC capture session for the given window.
    ///
    /// `on_frame` is called on every captured frame (WGC background thread).
    /// `on_drop` is called once when this [`Capturer`] is dropped (caller thread).
    /// Pass `|| {}` for either parameter when no instrumentation is needed.
    ///
    /// The yellow capture border is disabled via
    /// [`DrawBorderSettings::WithoutBorder`] (requires Windows 10 Build 20348+).
    ///
    /// # Errors
    ///
    /// Returns an error if the WGC session cannot be started (e.g., window not
    /// found, unsupported OS version, or API failure).
    #[instrument(skip(on_frame, on_drop))]
    pub fn start(
        hwnd: HWND,
        on_frame: impl Fn() + Send + Sync + 'static,
        on_drop: impl Fn() + Send + 'static,
    ) -> Result<Self> {
        let window = Window::from_raw_hwnd(hwnd.0);

        let ctx = Arc::new(HandlerContext {
            latest_frame: Mutex::new(None),
            on_frame: Box::new(on_frame),
        });

        let settings = Settings::new(
            window,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            Arc::clone(&ctx),
        );

        let control = Handler::start_free_threaded(settings)
            .context("failed to start WGC capture session")?;

        debug!(?hwnd, "WGC capture session started");

        Ok(Self {
            hwnd,
            ctx,
            control: Some(control),
            on_drop: Box::new(on_drop),
        })
    }
}

impl Capture for Capturer {
    #[instrument(skip(self))]
    fn capture_frame(&mut self) -> Result<RgbaImage> {
        let mut latest = self
            .ctx
            .latest_frame
            .lock()
            .map_err(|e| anyhow::anyhow!("shared frame state lock poisoned: {e}"))?;

        match latest.take() {
            Some(frame) => Ok(frame),
            None => bail!("no frame available yet from WGC"),
        }
    }

    fn is_window_alive(&self) -> bool {
        window::is_window_alive(self.hwnd)
    }
}
