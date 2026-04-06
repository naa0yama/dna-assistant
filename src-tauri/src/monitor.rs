//! Background monitor loop: capture → detect → notify.
//!
//! Runs on a dedicated thread, capturing game frames at regular intervals,
//! feeding them through the detection pipeline, and sending notifications
//! when trigger conditions are met.

// On Linux, only `MonitorState` (empty unit struct) is compiled.
// All types, configuration, and the monitor loop are Windows-only.

/// Minimal monitor state for non-Windows (only needed for `tauri::manage()`).
#[cfg(not(target_os = "windows"))]
#[derive(Debug, Default)]
pub struct MonitorState;

#[cfg(not(target_os = "windows"))]
impl MonitorState {
    /// Create a new stub state.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

// ---------- Windows implementation ----------

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::thread;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use anyhow::{Context as _, Result};
#[cfg(target_os = "windows")]
use dna_capture::Capture;
#[cfg(target_os = "windows")]
use dna_detector::config::DetectionConfig;
#[cfg(target_os = "windows")]
use dna_detector::detector::Detector;
#[cfg(target_os = "windows")]
use dna_detector::detector::dialog::DialogDetector;
#[cfg(target_os = "windows")]
use dna_detector::detector::result::ResultScreenDetector;
#[cfg(target_os = "windows")]
use dna_detector::detector::round::RoundDetector;
#[cfg(target_os = "windows")]
use dna_detector::event::DetectionEvent;
#[cfg(target_os = "windows")]
use dna_detector::titlebar::crop_titlebar;
#[cfg(target_os = "windows")]
use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "windows")]
use tracing::{debug, error, info, instrument, trace, warn};

#[cfg(all(target_os = "windows", feature = "otel"))]
use opentelemetry::KeyValue;

#[cfg(target_os = "windows")]
use crate::notification::NotificationManager;

#[cfg(target_os = "windows")]
/// Monitor loop configuration.
///
/// Capture/detection timing fields are serialized as **milliseconds** (u64).
/// Notification timing fields are serialized as **seconds** (f64).
#[allow(clippy::struct_excessive_bools)] // Settings struct with many toggles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// Capture interval between frames (ms).
    #[serde(with = "serde_duration_ms")]
    pub capture_interval: Duration,
    /// Interval between window search retries (ms).
    #[serde(with = "serde_duration_ms")]
    pub window_search_interval: Duration,
    /// Maximum consecutive capture failures before re-searching window.
    pub max_capture_retries: u32,
    /// Detection page preview refresh interval (ms).
    #[serde(with = "serde_duration_ms")]
    pub preview_interval: Duration,
    /// Cooldown between duplicate notifications (sec).
    #[serde(with = "serde_duration_secs")]
    pub notification_cooldown: Duration,
    /// Sustain for `DialogVisible` before notification (sec).
    #[serde(with = "serde_duration_secs")]
    pub notify_dialog_sustain: Duration,
    /// Sustain for `RoundGone` before notification (sec).
    #[serde(with = "serde_duration_secs")]
    pub notify_round_sustain: Duration,
    /// Cooldown between repeated round-completion notifications (sec).
    #[serde(with = "serde_duration_secs")]
    pub notify_round_cooldown: Duration,
    /// Whether the Round detector is enabled.
    #[serde(default = "default_true")]
    pub round_enabled: bool,
    /// Whether the Dialog detector is enabled.
    #[serde(default = "default_true")]
    pub dialog_enabled: bool,
    /// Sliding window duration for transition confirmation (sec).
    #[serde(default = "default_confirmation_window", with = "serde_duration_secs")]
    pub confirmation_window: Duration,
    /// Ratio of agreeing frames required within the window (0.0-1.0).
    #[serde(default = "default_confirmation_ratio")]
    pub confirmation_ratio: f64,
    /// Master toggle for all notifications.
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    /// Whether to notify on `RoundGone` events.
    #[serde(default = "default_true")]
    pub notify_round_enabled: bool,
    /// Whether to notify on `DialogVisible` events.
    #[serde(default = "default_true")]
    pub notify_dialog_enabled: bool,
    /// Whether to notify on `ResultScreen` events.
    #[serde(default = "default_true")]
    pub notify_result_enabled: bool,
    /// `RoundTrip` Green threshold (sec). Below = normal.
    #[serde(default = "default_roundtrip_green", with = "serde_duration_secs")]
    pub roundtrip_green: Duration,
    /// `RoundTrip` Yellow threshold (sec). Above Green = warning.
    #[serde(default = "default_roundtrip_yellow", with = "serde_duration_secs")]
    pub roundtrip_yellow: Duration,
    /// `RoundTrip` Red threshold (sec). Above Yellow = alert.
    #[serde(default = "default_roundtrip_red", with = "serde_duration_secs")]
    pub roundtrip_red: Duration,
    /// Notify when `RoundTrip` exceeds Green threshold.
    #[serde(default)]
    pub notify_roundtrip_green: bool,
    /// Notify when `RoundTrip` exceeds Yellow threshold.
    #[serde(default)]
    pub notify_roundtrip_yellow: bool,
    /// Notify when `RoundTrip` exceeds Red threshold.
    #[serde(default = "default_true")]
    pub notify_roundtrip_red: bool,
    /// Maximum number of repeat notifications (shared by `RoundTrip` and `CaptureLost`).
    #[serde(
        default = "default_notification_max_repeat",
        alias = "roundtrip_max_repeat"
    )]
    pub notification_max_repeat: u32,
    /// Whether to notify when capture frames stop arriving.
    #[serde(default = "default_true")]
    pub notify_capture_lost_enabled: bool,
    /// Sustain duration for capture-lost before notification (sec).
    #[serde(default = "default_capture_lost_sustain", with = "serde_duration_secs")]
    pub notify_capture_lost_sustain: Duration,
    /// Suppress notifications when the game window is the foreground window.
    #[serde(default)]
    pub suppress_when_game_focused: bool,
    /// Send notifications via Discord webhook instead of Windows toast.
    #[serde(default)]
    pub discord_enabled: bool,
    /// Discord webhook URL for notifications.
    #[serde(default)]
    pub discord_webhook_url: String,
    /// Discord user/role ID for mentions (e.g., "123456789012345678").
    #[serde(default)]
    pub discord_mention_id: String,
}

#[cfg(target_os = "windows")]
const fn default_true() -> bool {
    true
}

#[cfg(target_os = "windows")]
const fn default_confirmation_window() -> Duration {
    Duration::from_secs(3)
}

#[cfg(target_os = "windows")]
const fn default_roundtrip_green() -> Duration {
    Duration::from_secs(60)
}

#[cfg(target_os = "windows")]
const fn default_roundtrip_yellow() -> Duration {
    Duration::from_secs(120)
}

#[cfg(target_os = "windows")]
const fn default_roundtrip_red() -> Duration {
    Duration::from_secs(180)
}

#[cfg(target_os = "windows")]
const fn default_notification_max_repeat() -> u32 {
    5
}

#[cfg(target_os = "windows")]
const fn default_capture_lost_sustain() -> Duration {
    Duration::from_secs(5)
}

#[cfg(target_os = "windows")]
const fn default_confirmation_ratio() -> f64 {
    0.80
}

#[cfg(target_os = "windows")]
impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            capture_interval: Duration::from_millis(200),
            window_search_interval: Duration::from_millis(3000),
            max_capture_retries: 3,
            preview_interval: Duration::from_millis(200),
            notification_cooldown: Duration::from_secs(60),
            notify_dialog_sustain: Duration::from_secs(3),
            notify_round_sustain: Duration::from_secs(5),
            notify_round_cooldown: Duration::from_secs(10),
            round_enabled: true,
            dialog_enabled: true,
            confirmation_window: Duration::from_secs(3),
            confirmation_ratio: 0.80,
            notifications_enabled: true,
            notify_round_enabled: true,
            notify_dialog_enabled: true,
            notify_result_enabled: true,
            roundtrip_green: Duration::from_secs(60),
            roundtrip_yellow: Duration::from_secs(120),
            roundtrip_red: Duration::from_secs(180),
            notify_roundtrip_green: false,
            notify_roundtrip_yellow: false,
            notify_roundtrip_red: true,
            notification_max_repeat: 5,
            notify_capture_lost_enabled: true,
            notify_capture_lost_sustain: Duration::from_secs(5),
            suppress_when_game_focused: false,
            discord_enabled: false,
            discord_webhook_url: String::new(),
            discord_mention_id: String::new(),
        }
    }
}

#[cfg(target_os = "windows")]
impl MonitorConfig {
    /// Whether Discord webhook notifications are configured and enabled.
    pub const fn is_discord_active(&self) -> bool {
        self.discord_enabled && !self.discord_webhook_url.is_empty()
    }
}

#[cfg(target_os = "windows")]
/// Serialize/deserialize `Duration` as milliseconds (u64).
mod serde_duration_ms {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis().try_into().unwrap_or(u64::MAX))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        // ast-grep: serde deserializers use serde::de::Error, not anyhow
        #[allow(clippy::question_mark)]
        let ms = match u64::deserialize(d) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        Ok(Duration::from_millis(ms))
    }
}

#[cfg(target_os = "windows")]
/// Serialize/deserialize `Duration` as seconds (f64).
mod serde_duration_secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.as_secs_f64())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        #[allow(clippy::question_mark)]
        let secs = match f64::deserialize(d) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        Ok(Duration::from_secs_f64(secs))
    }
}

// --- Windows-only implementation below ---
// On Linux, only the types above (MonitorConfig, MonitorStatus, etc.) are compiled.
// The monitor loop, TransitionFilter, and helper functions require dna-capture
// and notify-rust which are Windows-only.

#[cfg(target_os = "windows")]
/// Granularity for interruptible sleeps (check stop flag every 200ms).
const SLEEP_GRANULARITY: Duration = Duration::from_millis(200);

#[cfg(target_os = "windows")]
/// Brightness threshold for OCR binarization of white text on dark backgrounds.
const OCR_BINARIZE_THRESHOLD: u8 = 140;

/// Current monitoring state.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitoringState {
    /// Not monitoring.
    Idle,
    /// Looking for game window.
    SearchingWindow,
    /// Actively capturing and detecting.
    Capturing,
}

/// Status snapshot for IPC queries.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Serialize)]
pub struct MonitorStatus {
    /// Current state.
    pub state: MonitoringState,
    /// Frames captured since last start.
    pub frames_captured: u64,
    /// Detection events since last start.
    pub events_detected: u64,
    /// Last event description.
    pub last_event: Option<String>,
    /// Last frame processing time in milliseconds (capture + detect).
    pub frame_time_ms: f64,
    /// Frames per second (rolling average).
    pub fps: f64,
    /// Whether OCR engine is available.
    pub ocr_available: bool,
    /// Resolution warning (shown when frame size is below recommended).
    pub resolution_warning: Option<String>,
}

#[cfg(target_os = "windows")]
impl Default for MonitorStatus {
    fn default() -> Self {
        Self {
            state: MonitoringState::Idle,
            frames_captured: 0,
            events_detected: 0,
            last_event: None,
            frame_time_ms: 0.0,
            fps: 0.0,
            ocr_available: false,
            resolution_warning: None,
        }
    }
}

/// Serializable detection event for the frontend.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Serialize)]
pub struct DetectionEventPayload {
    /// Event kind (e.g., "`RoundVisible`", "`DialogVisible`").
    pub kind: String,
    /// Human-readable detail.
    pub detail: String,
    /// Current round number (if known).
    pub round_number: Option<u32>,
    /// Elapsed time for the current round (e.g., "1m 23s").
    pub elapsed: Option<String>,
    /// Elapsed time in seconds (for frontend threshold comparison).
    pub elapsed_secs: Option<f64>,
}

// --- Windows-only: monitor loop, TransitionFilter, and helpers ---
#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    /// Detector state category for transition tracking.
    ///
    /// Each detector produces two complementary event kinds (e.g., `RoundVisible` /
    /// `RoundGone`). We track the last-seen kind per category and only forward
    /// events to the UI and notification manager when the kind changes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DetectorCategory {
        Round,
        Dialog,
        ResultScreen,
        RoundNumber,
    }

    /// Tracks last-seen event kind per detector category, forwarding only transitions.
    ///
    /// Uses a sliding-window approach: a new state must appear in at least
    /// `confirmation_ratio` of the last `window_size` frames before being
    /// confirmed. This tolerates occasional OCR/detection noise.
    #[derive(Debug)]
    struct TransitionFilter {
        /// Confirmed state per category.
        state: [Option<&'static str>; 4],
        /// Sliding window of recent event kinds per category.
        history: [std::collections::VecDeque<&'static str>; 4],
        /// Number of frames in the sliding window.
        window_size: usize,
        /// Ratio threshold (0.0-1.0) for confirmation.
        confirmation_ratio: f64,
    }

    impl TransitionFilter {
        fn new(window_size: usize, confirmation_ratio: f64) -> Self {
            Self {
                state: [None; 4],
                history: std::array::from_fn(|_| {
                    std::collections::VecDeque::with_capacity(window_size)
                }),
                window_size,
                confirmation_ratio,
            }
        }

        /// Returns `true` if this event represents a confirmed state change.
        fn is_transition(&mut self, event: &DetectionEvent) -> bool {
            // Round number events are internal-only (update round state
            // but don't appear in the UI event log). Always suppress here.
            if matches!(event, DetectionEvent::RoundSelectScreen { .. }) {
                return false;
            }

            #[allow(clippy::as_conversions)] // enum to usize is safe
            let idx = categorize(event) as usize;
            let kind = event_kind_name(event);

            // Push into sliding window
            #[allow(clippy::indexing_slicing)] // idx is bounded by enum variant count
            let history = &mut self.history[idx];
            history.push_back(kind);
            while history.len() > self.window_size {
                history.pop_front();
            }

            #[allow(clippy::indexing_slicing)]
            let confirmed = &mut self.state[idx];

            // Already in this state — no transition
            if *confirmed == Some(kind) {
                return false;
            }

            // Need a full window before confirming any transition
            if history.len() < self.window_size {
                return false;
            }

            // Count how many frames in the window agree with this kind
            let agree_count = history.iter().filter(|&&k| k == kind).count();
            #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
            let ratio = (agree_count as f64) / (self.window_size as f64);

            if ratio >= self.confirmation_ratio {
                *confirmed = Some(kind);
                true
            } else {
                false
            }
        }

        /// Check if any event in the list would be a potential transition.
        fn has_pending_transition(&self, events: &[DetectionEvent]) -> bool {
            events.iter().any(|e| {
                #[allow(clippy::as_conversions)]
                let idx = categorize(e) as usize;
                let kind = event_kind_name(e);
                self.state.get(idx).copied().flatten() != Some(kind)
            })
        }
    }

    /// Map a detection event to its detector category.
    const fn categorize(event: &DetectionEvent) -> DetectorCategory {
        match event {
            DetectionEvent::RoundVisible { .. } | DetectionEvent::RoundGone { .. } => {
                DetectorCategory::Round
            }
            DetectionEvent::DialogVisible { .. } | DetectionEvent::DialogGone { .. } => {
                DetectorCategory::Dialog
            }
            DetectionEvent::ResultScreenVisible { .. }
            | DetectionEvent::ResultScreenGone { .. } => DetectorCategory::ResultScreen,
            DetectionEvent::RoundSelectScreen { .. } => DetectorCategory::RoundNumber,
        }
    }

    /// Handle for controlling the monitor thread.
    pub struct MonitorHandle {
        pub stop_flag: Arc<AtomicBool>,
        pub thread: thread::JoinHandle<()>,
    }

    /// Capture metadata for the Detection page.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct CaptureInfo {
        /// Target window title.
        pub window_name: String,
        /// Frame dimensions (width x height).
        pub width: u32,
        /// Frame height.
        pub height: u32,
        /// Active capture backend name.
        pub backend: String,
    }

    /// Latest captured frame, shared with IPC.
    /// Uses `Arc` to avoid cloning the full image buffer every frame.
    /// PNG encoding is deferred to `get_capture_preview` IPC request time.
    #[derive(Debug, Default)]
    pub struct LatestFrame {
        /// Shared reference to the latest frame (zero-copy store from capture loop).
        pub image: Option<Arc<image::RgbaImage>>,
        /// Capture metadata.
        pub info: CaptureInfo,
    }

    /// Shared monitor state managed by Tauri.
    pub struct MonitorState {
        #[cfg(target_os = "windows")]
        pub handle: Mutex<Option<platform::MonitorHandle>>,
        pub status: Arc<Mutex<MonitorStatus>>,
        pub latest_frame: Arc<Mutex<LatestFrame>>,
        pub config: Arc<Mutex<MonitorConfig>>,
    }

    impl MonitorState {
        /// Create with default config (call `load_config` after `AppHandle` is available).
        #[must_use]
        pub fn new() -> Self {
            Self {
                #[cfg(target_os = "windows")]
                handle: Mutex::new(None),
                status: Arc::new(Mutex::new(MonitorStatus::default())),
                latest_frame: Arc::new(Mutex::new(LatestFrame::default())),
                config: Arc::new(Mutex::new(MonitorConfig::default())),
            }
        }

        /// Load persisted config from disk, replacing the default.
        pub fn load_config(&self, app_handle: &tauri::AppHandle) {
            let loaded = crate::settings::load(app_handle);
            if let Ok(mut cfg) = self.config.lock() {
                *cfg = loaded;
            }
        }
    }

    impl std::fmt::Debug for MonitorState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("MonitorState").finish_non_exhaustive()
        }
    }

    /// Start the monitor loop on a background thread.
    ///
    /// # Errors
    ///
    /// Returns an error if monitoring is already active.
    pub fn start(app_handle: AppHandle, state: &MonitorState) -> Result<()> {
        let mut handle_guard = state
            .handle
            .lock()
            .map_err(|e| anyhow::anyhow!("monitor state lock poisoned: {e}"))?;

        if handle_guard.is_some() {
            anyhow::bail!("monitoring is already active");
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();
        let status = state.status.clone();
        let latest_frame = state.latest_frame.clone();
        let monitor_config = state.config.lock().map(|c| c.clone()).unwrap_or_default();

        let thread = thread::Builder::new()
            .name("monitor-loop".into())
            .spawn(move || {
                monitor_loop(
                    app_handle,
                    stop_flag_clone,
                    status,
                    latest_frame,
                    monitor_config,
                );
            })
            .context("failed to spawn monitor thread")?;

        *handle_guard = Some(MonitorHandle { stop_flag, thread });
        drop(handle_guard);

        info!("monitor started");
        Ok(())
    }

    /// Stop the monitor loop and wait for the thread to finish.
    pub fn stop(app_handle: &AppHandle, state: &MonitorState) {
        let mut handle_guard = match state.handle.lock() {
            Ok(g) => g,
            Err(e) => {
                warn!(%e, "monitor state lock poisoned on stop");
                return;
            }
        };

        let handle = handle_guard.take();
        drop(handle_guard);

        if let Some(handle) = handle {
            handle.stop_flag.store(true, Ordering::Relaxed);
            if let Err(e) = handle.thread.join() {
                warn!(?e, "monitor thread panicked");
            }
            info!("monitor stopped");
        }

        if let Ok(mut status) = state.status.lock() {
            status.state = MonitoringState::Idle;
        }
        emit_status(app_handle, &state.status);
    }

    /// Main monitor loop running on a background thread.
    #[instrument(skip_all, name = "monitor_loop")]
    #[allow(clippy::needless_pass_by_value)] // args are moved from thread::spawn closure
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)] // loop body is inherently sequential
    fn monitor_loop(
        app_handle: AppHandle,
        stop_flag: Arc<AtomicBool>,
        status: Arc<Mutex<MonitorStatus>>,
        latest_frame: Arc<Mutex<LatestFrame>>,
        monitor_config: MonitorConfig,
    ) {
        let det_config = DetectionConfig::default();
        let mut notification_mgr = NotificationManager::new(&monitor_config);
        notification_mgr.set_latest_frame(latest_frame.clone());
        let mut capture_fail_since: Option<Instant> = None;
        // Compute window size: confirmation_window / capture_interval, at least 1
        let window_size = monitor_config
            .confirmation_window
            .as_millis()
            .checked_div(monitor_config.capture_interval.as_millis().max(1))
            .unwrap_or(1)
            .max(1);
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        let window_size = window_size as usize;
        info!(
            window_size,
            confirmation_ratio = monitor_config.confirmation_ratio,
            "transition filter: sliding window"
        );
        let mut transition_filter =
            TransitionFilter::new(window_size, monitor_config.confirmation_ratio);

        // Round state tracking
        let mut current_round: Option<u32> = None;
        let mut round_start: Option<Instant> = None;
        // Round number votes from OCR frames (cleared on RoundVisible)
        let mut select_round_votes: Vec<u32> = Vec::new();
        // Recent confirmed round numbers (max 3) for consecutive inference
        let mut round_history: Vec<u32> = Vec::with_capacity(3);
        // Cached resolution warning to avoid mutex lock every frame
        let mut prev_resolution_warning: Option<String> = None;
        // Result screen scanning: activated after RoundGone confirmation
        let mut result_scanning = false;
        // Whether ResultScreenVisible has been confirmed (gates Gone events)
        let mut result_visible_confirmed = false;

        // Build detectors.
        // All detectors call analyze() directly. TransitionFilter handles
        // state-change deduplication.
        let round_number_rois = dna_detector::config::RoundNumberRoiConfig::default();
        let round_detector = RoundDetector::new(det_config.round.clone());
        let dialog_detector = DialogDetector::new(det_config.dialog.clone());
        let result_detector =
            ResultScreenDetector::new(dna_detector::config::ResultScreenRoiConfig::default());

        // Initialize OCR engine (optional — gracefully degrades if unavailable)
        let ocr_engine = match dna_capture::ocr::JapaneseOcrEngine::new() {
            Ok(engine) => {
                info!("Windows OCR engine initialized (Japanese)");
                Some(engine)
            }
            Err(e) => {
                warn!(%e, "OCR unavailable — falling back to pixel-only detection");
                None
            }
        };

        update_status(&status, |s| s.ocr_available = ocr_engine.is_some());

        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            update_status(&status, |s| s.state = MonitoringState::SearchingWindow);
            emit_status(&app_handle, &status);

            // Find game window
            let hwnd = loop {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                match dna_capture::window::find_game() {
                    Ok(hwnd) => break hwnd,
                    Err(e) => {
                        debug!(%e, "game window not found, retrying");
                        if interruptible_sleep(monitor_config.window_search_interval, &stop_flag) {
                            return;
                        }
                    }
                }
            };

            info!(?hwnd, "game window found, initializing capture");
            update_status(&status, |s| s.state = MonitoringState::Capturing);
            emit_status(&app_handle, &status);

            // Build WGC metric callbacks so dna-capture stays OTel-free.
            #[cfg(feature = "otel")]
            let (wgc_on_frame, wgc_on_drop) = match crate::metrics::get() {
                Some(m) => {
                    let frames = m.wgc_frames_received.clone();
                    let dropped = m.wgc_capturer_dropped.clone();
                    (
                        Box::new(move || frames.add(1, &[]))
                            as Box<dyn Fn() + Send + Sync + 'static>,
                        Box::new(move || dropped.add(1, &[])) as Box<dyn Fn() + Send + 'static>,
                    )
                }
                None => (
                    Box::new(|| {}) as Box<dyn Fn() + Send + Sync + 'static>,
                    Box::new(|| {}) as Box<dyn Fn() + Send + 'static>,
                ),
            };
            #[cfg(not(feature = "otel"))]
            let (wgc_on_frame, wgc_on_drop) = (
                Box::new(|| {}) as Box<dyn Fn() + Send + Sync + 'static>,
                Box::new(|| {}) as Box<dyn Fn() + Send + 'static>,
            );

            // Try WGC first, fall back to PrintWindow
            let (mut capturer, backend_name): (Box<dyn Capture>, &str) =
                match dna_capture::wgc::Capturer::start(hwnd, wgc_on_frame, wgc_on_drop) {
                    Ok(c) => {
                        info!("using WGC capture backend");
                        #[cfg(feature = "otel")]
                        if let Some(m) = crate::metrics::get() {
                            m.wgc_capturer_started.add(1, &[]);
                        }
                        (Box::new(c), "WGC")
                    }
                    Err(e) => {
                        warn!(%e, "WGC failed, falling back to PrintWindow");
                        (
                            Box::new(dna_capture::printwindow::Capturer::new(hwnd)),
                            "PrintWindow",
                        )
                    }
                };

            let mut consecutive_failures: u32 = 0;
            let mut had_successful_capture = false;
            let capture_info = CaptureInfo {
                window_name: String::from(dna_capture::window::GAME_WINDOW_TITLE),
                width: 0,
                height: 0,
                backend: String::from(backend_name),
            };

            // Capture loop
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }

                let frame_start = Instant::now();

                // Capture frame
                let frame = match capturer.capture_frame() {
                    Ok(f) => {
                        if capture_fail_since.take().is_some() {
                            notification_mgr.reset_capture_lost();
                        }
                        consecutive_failures = 0;
                        had_successful_capture = true;
                        #[cfg(feature = "otel")]
                        if let Some(m) = crate::metrics::get() {
                            m.capture_frames.add(1, &[]);
                            m.capture_duration
                                .record(frame_start.elapsed().as_secs_f64(), &[]);
                        }
                        f
                    }
                    Err(e) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        warn!(%e, consecutive_failures, "capture failed");
                        if had_successful_capture {
                            let fail_start = *capture_fail_since.get_or_insert_with(Instant::now);
                            if fail_start.elapsed() >= monitor_config.notify_capture_lost_sustain {
                                notification_mgr.notify_capture_lost();
                            }
                        }
                        if consecutive_failures >= monitor_config.max_capture_retries {
                            error!("max capture retries exceeded, re-searching window");
                            break; // outer loop will re-search
                        }
                        if interruptible_sleep(monitor_config.capture_interval, &stop_flag) {
                            return;
                        }
                        continue;
                    }
                };

                // Wrap in Arc (zero-copy move) for shared access with IPC
                let frame = Arc::new(frame);

                // Check resolution every frame (game window can be resized)
                let warning = check_resolution(frame.width(), frame.height());
                if warning != prev_resolution_warning {
                    if let Some(ref msg) = warning {
                        warn!(%msg, width = frame.width(), height = frame.height(), "resolution warning");
                    }
                    prev_resolution_warning = warning;
                    update_status(&status, |s| {
                        s.resolution_warning.clone_from(&prev_resolution_warning);
                    });
                    emit_status(&app_handle, &status);
                }

                // Store latest frame for Detection page preview
                store_latest_frame(&latest_frame, frame.clone(), &capture_info);

                // Crop titlebar
                let game_frame = crop_titlebar(&frame);

                // Run pixel detectors (skip disabled ones)
                let mut raw_events: Vec<DetectionEvent> = Vec::new();
                if monitor_config.round_enabled {
                    raw_events.extend(round_detector.analyze(&game_frame));
                }
                if monitor_config.dialog_enabled {
                    raw_events.extend(dialog_detector.analyze(&game_frame));
                }

                // OCR-assisted detection (Phase 2)
                // Only run OCR when pixel detector state changes (transition)
                // to avoid expensive OCR calls every frame.
                if let Some(ref ocr_engine) = ocr_engine
                    && transition_filter.has_pending_transition(&raw_events)
                {
                    run_ocr(ocr_engine, &game_frame, &mut raw_events, &det_config);
                }

                // Round number OCR (independent of pixel detector transitions).
                // Scans for round selection screens to extract round numbers.
                if monitor_config.round_enabled
                    && let Some(ref ocr_engine) = ocr_engine
                {
                    #[cfg(feature = "otel")]
                    let ocr_start = Instant::now();
                    run_round_number_ocr(
                        ocr_engine,
                        &game_frame,
                        &mut raw_events,
                        &round_number_rois,
                    );
                    #[cfg(feature = "otel")]
                    if let Some(m) = crate::metrics::get() {
                        m.ocr_calls.add(1, &[KeyValue::new("kind", "round_number")]);
                        m.ocr_duration.record(
                            ocr_start.elapsed().as_secs_f64(),
                            &[KeyValue::new("kind", "round_number")],
                        );
                    }
                }

                // Result screen OCR: runs every frame while result_scanning is active.
                // Gated by RoundGone → result_scanning=true chain.
                if result_scanning && let Some(ref ocr_engine) = ocr_engine {
                    #[cfg(feature = "otel")]
                    let ocr_start = Instant::now();
                    let result_events = result_detector.analyze(&game_frame, ocr_engine);
                    #[cfg(feature = "otel")]
                    if let Some(m) = crate::metrics::get() {
                        m.ocr_calls
                            .add(1, &[KeyValue::new("kind", "result_screen")]);
                        m.ocr_duration.record(
                            ocr_start.elapsed().as_secs_f64(),
                            &[KeyValue::new("kind", "result_screen")],
                        );
                    }
                    for event in result_events {
                        match event {
                            // Always pass through Visible
                            DetectionEvent::ResultScreenVisible { .. } => {
                                result_visible_confirmed = true;
                                raw_events.push(event);
                            }
                            // Only pass Gone if Visible was already confirmed
                            // (avoids false None→Gone transition on scan start)
                            DetectionEvent::ResultScreenGone { .. } => {
                                if result_visible_confirmed {
                                    raw_events.push(event);
                                }
                            }
                            _ => raw_events.push(event),
                        }
                    }
                }

                // Collect round numbers and manage result scanning from raw events.
                for event in &raw_events {
                    if let DetectionEvent::RoundSelectScreen {
                        completed_round: Some(done),
                        ..
                    } = event
                    {
                        select_round_votes.push(*done);
                        #[cfg(feature = "otel")]
                        if let Some(m) = crate::metrics::get() {
                            m.select_votes_pushes.add(1, &[]);
                            m.select_votes_len.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }

                // Notification uses raw events (needs sustained-condition tracking)
                if !raw_events.is_empty() {
                    notification_mgr.set_current_round(current_round);
                    notification_mgr.process_events(&raw_events);
                }

                // RoundTrip threshold check: fires during RoundVisible
                // based on elapsed time since round_start.
                if let Some(start) = round_start {
                    notification_mgr.notify_roundtrip(start.elapsed());
                }

                // Filter to state transitions only for UI
                let transition_events: Vec<DetectionEvent> = raw_events
                    .into_iter()
                    .filter(|e| transition_filter.is_transition(e))
                    .collect();

                if !transition_events.is_empty() {
                    // Save debug frame at trace level for false-positive investigation
                    if tracing::enabled!(tracing::Level::TRACE) {
                        save_debug_frame(&frame, &transition_events);
                    }

                    let event_count = u64::try_from(transition_events.len()).unwrap_or(u64::MAX);

                    update_status(&status, |s| {
                        s.events_detected = s.events_detected.saturating_add(event_count);
                        s.last_event = transition_events.last().map(event_description);
                    });

                    // Emit transitions to frontend
                    for event in &transition_events {
                        #[cfg(feature = "otel")]
                        if let Some(m) = crate::metrics::get() {
                            m.detection_events
                                .add(1, &[KeyValue::new("kind", event_kind_name(event))]);
                        }
                        let mut elapsed = None;
                        #[allow(unused_mut)]
                        let mut elapsed_duration: Option<Duration> = None;

                        match event {
                            DetectionEvent::RoundVisible { .. } => {
                                if round_start.is_none() {
                                    round_start = Some(Instant::now());
                                }
                                // Reset RoundTrip notifications for new round
                                notification_mgr.reset_roundtrip();
                                // Clear stale OCR data from previous cycle
                                select_round_votes.clear();
                                // New round started: stop result scanning
                                result_scanning = false;
                                #[cfg(feature = "otel")]
                                if let Some(m) = crate::metrics::get() {
                                    m.select_votes_clears.add(1, &[]);
                                    m.select_votes_len.store(0, Ordering::Relaxed);
                                    m.result_scanning.store(false, Ordering::Relaxed);
                                }
                            }
                            DetectionEvent::RoundGone { .. } => {
                                // Calculate elapsed time for this round
                                let elapsed_duration = round_start.map(|s| s.elapsed());
                                elapsed = elapsed_duration.map(format_elapsed);
                                round_start = None;

                                // Resolve completed round number from OCR sources
                                let majority = majority_vote(
                                    &select_round_votes,
                                    monitor_config.confirmation_ratio,
                                );
                                select_round_votes.clear();
                                #[cfg(feature = "otel")]
                                if let Some(m) = crate::metrics::get() {
                                    m.select_votes_clears.add(1, &[]);
                                    m.select_votes_len.store(0, Ordering::Relaxed);
                                }
                                let completed =
                                    resolve_round_number(majority, current_round, &round_history);
                                if let Some(num) = completed {
                                    current_round = Some(num);
                                    // Update history (keep last 3)
                                    round_history.push(num);
                                    if round_history.len() > 3 {
                                        round_history.remove(0);
                                    }
                                }
                                // Advance to next round after emitting
                                // (deferred: update current_round after payload)

                                // Start scanning for result screen
                                result_scanning = true;
                                #[cfg(feature = "otel")]
                                if let Some(m) = crate::metrics::get() {
                                    m.result_scanning.store(true, Ordering::Relaxed);
                                }
                            }
                            DetectionEvent::ResultScreenVisible { .. } => {
                                elapsed_duration = round_start.map(|s| s.elapsed());
                                elapsed = elapsed_duration.map(format_elapsed);
                                // Quest complete: reset round state
                                round_start = None;
                                current_round = None;
                                // Keep result_scanning=true until Gone confirms
                                // Notify ResultScreen (confirmed by TransitionFilter)
                                notification_mgr.notify_result_screen();
                            }
                            DetectionEvent::ResultScreenGone { .. } => {
                                // Result screen dismissed: full reset
                                result_scanning = false;
                                result_visible_confirmed = false;
                                #[cfg(feature = "otel")]
                                if let Some(m) = crate::metrics::get() {
                                    m.result_scanning.store(false, Ordering::Relaxed);
                                }
                                round_start = None;
                                notification_mgr.reset_roundtrip();
                            }
                            _ => {}
                        }

                        let payload = DetectionEventPayload {
                            kind: event_kind_name(event).into(),
                            detail: event_description(event),
                            round_number: current_round,
                            elapsed_secs: elapsed_duration.map(|d| d.as_secs_f64()),
                            elapsed,
                        };
                        let _ = app_handle.emit("detection-event", &payload);

                        // After emitting RoundGone, advance to next round
                        if matches!(event, DetectionEvent::RoundGone { .. }) {
                            if let Some(num) = current_round {
                                let next = num.saturating_add(1);
                                current_round = if next <= 99 { Some(next) } else { None };
                            }
                            round_start = Some(Instant::now());
                        }
                    }
                }

                // Update frame timing + emit status once per frame
                let frame_elapsed = frame_start.elapsed();
                let frame_ms = frame_elapsed.as_secs_f64().mul_add(1000.0, 0.0);
                #[cfg(feature = "otel")]
                if let Some(m) = crate::metrics::get() {
                    m.monitor_loop_iterations.add(1, &[]);
                    m.monitor_loop_duration
                        .record(frame_elapsed.as_secs_f64(), &[]);
                }
                let interval_ms = monitor_config
                    .capture_interval
                    .as_secs_f64()
                    .mul_add(1000.0, 0.0);
                let total_cycle = frame_ms + interval_ms;
                let fps_now = if total_cycle > 0.0 {
                    1000.0 / total_cycle
                } else {
                    0.0
                };
                update_status(&status, |s| {
                    s.frames_captured = s.frames_captured.saturating_add(1);
                    s.frame_time_ms = frame_ms;
                    s.fps = if s.fps == 0.0 {
                        fps_now
                    } else {
                        s.fps.mul_add(0.7, fps_now * 0.3)
                    };
                });
                emit_status(&app_handle, &status);

                // Check window alive
                if !capturer.is_window_alive() {
                    info!("game window closed, re-searching");
                    break; // outer loop will re-search
                }

                if interruptible_sleep(monitor_config.capture_interval, &stop_flag) {
                    return;
                }
            }
            // capturer is dropped here; the WGC Drop impl fires on_drop + stop().
        }
    }

    /// Update status under lock.
    fn update_status(status: &Arc<Mutex<MonitorStatus>>, f: impl FnOnce(&mut MonitorStatus)) {
        if let Ok(mut s) = status.lock() {
            f(&mut s);
        }
    }

    /// Emit current status to the frontend.
    fn emit_status(app_handle: &AppHandle, status: &Arc<Mutex<MonitorStatus>>) {
        if let Ok(s) = status.lock() {
            let _ = app_handle.emit("monitor-status", &*s);
        }
    }

    /// Get a short kind name for a detection event.
    const fn event_kind_name(event: &DetectionEvent) -> &'static str {
        match event {
            DetectionEvent::RoundVisible { .. } => "RoundVisible",
            DetectionEvent::RoundGone { .. } => "RoundGone",
            DetectionEvent::ResultScreenVisible { .. } => "ResultScreenVisible",
            DetectionEvent::ResultScreenGone { .. } => "ResultScreenGone",
            DetectionEvent::DialogVisible { .. } => "DialogVisible",
            DetectionEvent::DialogGone { .. } => "DialogGone",
            DetectionEvent::RoundSelectScreen { .. } => "RoundSelectScreen",
        }
    }

    /// Sleep for `duration` but check the stop flag every [`SLEEP_GRANULARITY`].
    /// Returns `true` if stop was requested during the sleep.
    fn interruptible_sleep(duration: Duration, stop_flag: &AtomicBool) -> bool {
        let mut remaining = duration;
        while remaining > Duration::ZERO {
            let chunk = remaining.min(SLEEP_GRANULARITY);
            thread::sleep(chunk);
            if stop_flag.load(Ordering::Relaxed) {
                return true;
            }
            remaining = remaining.saturating_sub(chunk);
        }
        false
    }

    /// Minimum recommended width for reliable OCR detection.
    const MIN_OCR_WIDTH: u32 = 1600;

    /// Known tested frame sizes (width x height, including titlebar).
    const TESTED_RESOLUTIONS: &[(u32, u32)] =
        &[(1282, 752), (1368, 800), (1602, 932), (1922, 1112)];

    /// Check frame resolution and return a warning if below recommended.
    fn check_resolution(width: u32, height: u32) -> Option<String> {
        let is_known = TESTED_RESOLUTIONS
            .iter()
            .any(|&(w, h)| w == width && h == height);

        if width < MIN_OCR_WIDTH {
            Some(format!(
                "Resolution {width}x{height} is below recommended (1600x900+). OCR accuracy may be degraded."
            ))
        } else if !is_known {
            Some(format!(
                "Resolution {width}x{height} has not been tested. Detection may be inaccurate."
            ))
        } else {
            None
        }
    }

    /// Pick the confirmed value from frame-level OCR votes.
    ///
    /// Returns `Some(value)` if a single value meets the `confirmation_ratio`
    /// threshold. Returns `None` if no value has sufficient agreement.
    fn majority_vote(votes: &[u32], confirmation_ratio: f64) -> Option<u32> {
        if votes.is_empty() {
            return None;
        }
        // Count occurrences of each value
        let mut counts: Vec<(u32, usize)> = Vec::new();
        for &v in votes {
            if let Some(entry) = counts.iter_mut().find(|(val, _)| *val == v) {
                entry.1 = entry.1.saturating_add(1);
            } else {
                counts.push((v, 1));
            }
        }
        // Find the value with the most votes
        let (best_val, best_count) = counts.iter().copied().max_by_key(|&(_, c)| c)?;
        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let ratio = (best_count as f64) / (votes.len() as f64);
        if ratio >= confirmation_ratio {
            Some(best_val)
        } else {
            None
        }
    }

    /// Resolve the completed round number from OCR majority vote.
    ///
    /// Validation rules:
    /// 1. Matches `current` → accept (expected value)
    /// 2. First detection (`current` is None) → accept (bootstrapped by
    ///    majority vote which already filters single-frame noise)
    /// 3. Doesn't match → reject (OCR misread), fall through to inference
    /// 4. No OCR → infer from consecutive history (last + 1)
    fn resolve_round_number(
        select_round: Option<u32>,
        current: Option<u32>,
        history: &[u32],
    ) -> Option<u32> {
        if let Some(num) = select_round {
            // First detection: majority vote already confirmed → accept
            if current.is_none() {
                return Some(num);
            }
            // Matches expected current round → accept
            if current == Some(num) {
                return Some(num);
            }
            // Mismatch with current: OCR likely misread, reject
        }

        // Fallback: infer from history if rounds are consecutive
        // e.g., history [4, 5, 6] → next completed = 7
        if history.len() >= 2 {
            let is_consecutive = history.windows(2).all(|w| {
                w.first()
                    .and_then(|a| w.get(1).map(|b| a.saturating_add(1) == *b))
                    .unwrap_or(false)
            });
            if is_consecutive && let Some(last) = history.last() {
                let next = last.saturating_add(1);
                if next <= 99 {
                    return Some(next);
                }
            }
        }

        // No reliable source
        None
    }

    /// Format a duration as human-readable elapsed time (e.g., "1m 23s").
    fn format_elapsed(duration: Duration) -> String {
        let total_secs = duration.as_secs();
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        if mins > 0 {
            format!("{mins}m {secs:02}s")
        } else {
            format!("{secs}s")
        }
    }

    /// Get a human-readable description for a detection event.
    fn event_description(event: &DetectionEvent) -> String {
        match event {
            DetectionEvent::RoundVisible { .. } => String::from("ラウンド進行中"),
            DetectionEvent::RoundGone { .. } => String::from("ラウンド完了"),
            DetectionEvent::ResultScreenVisible { .. } => String::from("依頼完了 (リザルト)"),
            DetectionEvent::ResultScreenGone { .. } => String::from("リザルト終了"),
            DetectionEvent::DialogVisible { .. } => String::from("ダイアログ表示"),
            DetectionEvent::DialogGone { .. } => String::from("ダイアログ消失"),
            // Internal-only events (not shown in UI)
            DetectionEvent::RoundSelectScreen { .. } => String::new(),
        }
    }

    /// Run OCR conditionally based on pixel detector results.
    ///
    /// - When `RoundVisible`: OCR the round text ROI to extract round number.
    /// - When `DialogVisible`: OCR the dialog area for "Tips" title confirmation.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity, clippy::ptr_arg)]
    fn run_ocr(
        ocr_engine: &dna_capture::ocr::JapaneseOcrEngine,
        game_frame: &image::RgbaImage,
        raw_events: &mut Vec<DetectionEvent>,
        det_config: &DetectionConfig,
    ) {
        let has_round_visible = raw_events
            .iter()
            .any(|e| matches!(e, DetectionEvent::RoundVisible { .. }));
        let has_dialog_visible = raw_events
            .iter()
            .any(|e| matches!(e, DetectionEvent::DialogVisible { .. }));

        // Enrich RoundVisible with OCR round number
        // Use an enlarged ROI (wider + taller than pixel detector ROI)
        // so Windows OCR has enough text height to recognize characters.
        if has_round_visible && let Some(ocr_image) = det_config.round.roi.crop(game_frame) {
            let binarized =
                dna_capture::ocr::binarize_white_text(&ocr_image, OCR_BINARIZE_THRESHOLD);
            match ocr_engine.recognize_text(&binarized) {
                Ok(text) => {
                    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                    let has_round_text = normalized.contains("ラウンド");
                    let round_num = dna_detector::round_number::parse(&text);
                    debug!(
                        round_number = ?round_num,
                        has_round_text,
                        ocr_text = %text,
                        "round OCR result"
                    );

                    if has_round_text {
                        // Enrich RoundVisible with confirmed round number
                        for event in raw_events.iter_mut() {
                            if let DetectionEvent::RoundVisible { round_number, .. } = event {
                                *round_number = round_num;
                            }
                        }
                    } else {
                        // OCR says no round text — pixel detection was a false positive.
                        // Replace RoundVisible with RoundGone.
                        for event in raw_events.iter_mut() {
                            if let DetectionEvent::RoundVisible {
                                white_ratio,
                                timestamp,
                                ..
                            } = event
                            {
                                debug!("OCR overriding false RoundVisible → RoundGone");
                                *event = DetectionEvent::RoundGone {
                                    white_ratio: *white_ratio,
                                    timestamp: *timestamp,
                                };
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!(%e, "round text OCR failed — keeping pixel result");
                }
            }
        }

        // Gate DialogVisible via OCR: confirm "Tips" title is present.
        // Dark camera angles trigger false DialogVisible from pixel detection.
        if has_dialog_visible && let Some(ocr_image) = det_config.dialog.ocr_roi.crop(game_frame) {
            let binarized =
                dna_capture::ocr::binarize_white_text(&ocr_image, OCR_BINARIZE_THRESHOLD);
            match ocr_engine.recognize_text(&binarized) {
                Ok(text) => {
                    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                    let has_tips = normalized.contains("Tips") || normalized.contains("tips");
                    debug!(has_tips, ocr_text = %text, "dialog OCR result");

                    if !has_tips {
                        // OCR says no "Tips" — pixel detection was a false positive.
                        for event in raw_events.iter_mut() {
                            if let DetectionEvent::DialogVisible {
                                text_ratio,
                                bg_dark_ratio,
                                timestamp,
                                ..
                            } = event
                            {
                                debug!("OCR overriding false DialogVisible → DialogGone");
                                *event = DetectionEvent::DialogGone {
                                    text_ratio: *text_ratio,
                                    bg_dark_ratio: *bg_dark_ratio,
                                    timestamp: *timestamp,
                                };
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!(%e, "dialog OCR failed — keeping pixel result");
                }
            }
        }
    }

    /// Scan for round number screens via OCR.
    ///
    /// Checks two transient screens independently of pixel detectors:
    /// 1. "XX ラウンド終了" — large centered number (1-2 sec display)
    /// 2. Round selection — "自動周回中" header + round panels (3-5 sec)
    fn run_round_number_ocr(
        ocr_engine: &dna_capture::ocr::JapaneseOcrEngine,
        game_frame: &image::RgbaImage,
        raw_events: &mut Vec<DetectionEvent>,
        rois: &dna_detector::config::RoundNumberRoiConfig,
    ) {
        use dna_detector::round_number::{is_round_select_text, parse, parse_select_header};

        // Check for round selection screen via header
        if let Some(header_image) = rois.select_header.crop(game_frame) {
            let binarized =
                dna_capture::ocr::binarize_white_text(&header_image, OCR_BINARIZE_THRESHOLD);
            match ocr_engine.recognize_text(&binarized) {
                Ok(header_text) => {
                    let is_select = is_round_select_text(&header_text);
                    if !header_text.is_empty() {
                        debug!(is_select, ocr_text = %header_text, "round_select header ROI OCR");
                    }

                    if is_select {
                        // Extract round number from header "自動周回中（X/Y）"
                        let header_round = parse_select_header(&header_text);

                        // Also try right/left panel OCR
                        let mut next_round: Option<u32> = None;
                        let mut completed_round: Option<u32> = None;

                        if let Some(right_image) = rois.select_next_round.crop(game_frame) {
                            let bin = dna_capture::ocr::binarize_white_text(
                                &right_image,
                                OCR_BINARIZE_THRESHOLD,
                            );
                            if let Ok(text) = ocr_engine.recognize_text(&bin) {
                                next_round = parse(&text);
                                debug!(next_round = ?next_round, ocr_text = %text, "round_select right OCR");
                            }
                        }

                        if let Some(left_image) = rois.select_completed_round.crop(game_frame) {
                            let bin = dna_capture::ocr::binarize_white_text(
                                &left_image,
                                OCR_BINARIZE_THRESHOLD,
                            );
                            if let Ok(text) = ocr_engine.recognize_text(&bin) {
                                completed_round = parse(&text);
                                debug!(completed_round = ?completed_round, ocr_text = %text, "round_select left OCR");
                            }
                        }

                        // Use header round as completed_round fallback
                        if completed_round.is_none() {
                            completed_round = header_round;
                        }

                        debug!(
                            header_round = ?header_round,
                            next_round = ?next_round,
                            completed_round = ?completed_round,
                            "round_select result"
                        );

                        if next_round.is_some() || completed_round.is_some() {
                            raw_events.push(DetectionEvent::RoundSelectScreen {
                                next_round,
                                completed_round,
                                timestamp: Instant::now(),
                            });
                        }
                    }
                }
                Err(e) => {
                    debug!(%e, "round_select header ROI OCR failed");
                }
            }
        }
    }

    /// Store the frame as `Arc` in shared state (zero-copy move, no clone).
    /// PNG encoding is deferred to `get_capture_preview` IPC request.
    fn store_latest_frame(
        latest_frame: &Arc<Mutex<LatestFrame>>,
        frame: Arc<image::RgbaImage>,
        info: &CaptureInfo,
    ) {
        if let Ok(mut lf) = latest_frame.lock() {
            lf.info.width = frame.width();
            lf.info.height = frame.height();
            // Only update static fields if not yet set
            if lf.info.window_name.is_empty() {
                lf.info.window_name.clone_from(&info.window_name);
                lf.info.backend.clone_from(&info.backend);
            }
            lf.image = Some(frame);
        }
    }

    /// Minimum width for debug frames (qHD, the detection lower bound).
    const DEBUG_FRAME_WIDTH: u32 = 960;

    /// Save a downscaled capture frame to disk when detection events fire.
    ///
    /// Only called when `TRACE` level is enabled. Frames are saved as PNG to
    /// a `debug-frames/` directory next to the executable.
    fn save_debug_frame(frame: &image::RgbaImage, events: &[DetectionEvent]) {
        use image::imageops::FilterType;
        use std::path::PathBuf;

        // Build output directory
        let dir: PathBuf = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("debug-frames")))
            .unwrap_or_else(|| PathBuf::from("debug-frames"));

        if std::fs::create_dir_all(&dir).is_err() {
            warn!(?dir, "failed to create debug-frames directory");
            return;
        }

        // Downscale to minimum detection resolution (keep aspect ratio)
        let resized = if frame.width() > DEBUG_FRAME_WIDTH {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            let new_height = (f64::from(frame.height()) * f64::from(DEBUG_FRAME_WIDTH)
                / f64::from(frame.width())) as u32;
            image::imageops::resize(frame, DEBUG_FRAME_WIDTH, new_height, FilterType::Nearest)
        } else {
            frame.clone()
        };

        // Build filename: 20260329_173202_RoundGone.png
        let now = chrono_filename();
        let kinds: Vec<&str> = events.iter().map(event_kind_name).collect();
        let kinds_str = kinds.join("_");
        let filename = format!("{now}_{kinds_str}.png");
        let path = dir.join(&filename);

        match resized.save(&path) {
            Ok(()) => {
                let kinds_joined = kinds.join(", ");
                trace!(
                    path = %path.display(),
                    events = %kinds_joined,
                    original_size = %format!("{}x{}", frame.width(), frame.height()),
                    saved_size = %format!("{}x{}", resized.width(), resized.height()),
                    "debug frame saved"
                );
            }
            Err(e) => {
                warn!(%e, "failed to save debug frame");
            }
        }
    }

    /// Generate a filename-safe timestamp: `20260329_173202_123`.
    fn chrono_filename() -> String {
        use std::time::SystemTime;

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs();
        let millis = now.subsec_millis();

        // Convert epoch seconds to date/time components (UTC)
        let days = secs / 86400;
        let time_of_day = secs % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        // Simple date calculation from days since epoch
        let (year, month, day) = days_to_ymd(days);

        format!("{year:04}{month:02}{day:02}_{hours:02}{minutes:02}{seconds:02}_{millis:03}")
    }

    /// Convert days since Unix epoch to (year, month, day).
    #[allow(clippy::cast_possible_truncation, clippy::arithmetic_side_effects)]
    const fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
        // Shift epoch to 2000-03-01 for easier leap year handling
        days += 719_468;
        let era = days / 146_097;
        let doe = days - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        (y, m, d)
    }
} // mod platform

#[cfg(target_os = "windows")]
pub use platform::{CaptureInfo, LatestFrame, MonitorState, start, stop};
