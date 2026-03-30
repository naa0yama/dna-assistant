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
use dna_detector::detector::round::RoundDetector;
#[cfg(target_os = "windows")]
use dna_detector::detector::skill::SkillDetector;
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

#[cfg(target_os = "windows")]
use crate::notification::NotificationManager;

#[cfg(target_os = "windows")]
/// Monitor loop configuration.
///
/// Capture/detection timing fields are serialized as **milliseconds** (u64).
/// Notification timing fields are serialized as **seconds** (f64).
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
    /// Duration to suppress Skill events after `RoundGone` (sec).
    #[serde(with = "serde_duration_secs")]
    pub round_transition_suppress: Duration,
    /// Cooldown between duplicate notifications (sec).
    #[serde(with = "serde_duration_secs")]
    pub notification_cooldown: Duration,
    /// Sustain for `SkillGreyed` before notification (sec).
    #[serde(with = "serde_duration_secs")]
    pub notify_skill_sustain: Duration,
    /// Sustain for `DialogVisible` before notification (sec).
    #[serde(with = "serde_duration_secs")]
    pub notify_dialog_sustain: Duration,
    /// Sustain for `RoundGone` before notification (sec).
    #[serde(with = "serde_duration_secs")]
    pub notify_round_sustain: Duration,
    /// Cooldown between repeated round-completion notifications (sec).
    /// Shorter than `notification_cooldown` because each round matters.
    #[serde(with = "serde_duration_secs")]
    pub notify_round_cooldown: Duration,
    /// Sustain for `AllyHpLow` before notification (sec).
    #[serde(with = "serde_duration_secs")]
    pub notify_ally_hp_sustain: Duration,
}

#[cfg(target_os = "windows")]
impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            capture_interval: Duration::from_millis(2000),
            window_search_interval: Duration::from_millis(3000),
            max_capture_retries: 3,
            preview_interval: Duration::from_millis(3000),
            round_transition_suppress: Duration::from_secs(15),
            notification_cooldown: Duration::from_secs(60),
            notify_skill_sustain: Duration::from_secs(5),
            notify_dialog_sustain: Duration::from_secs(3),
            notify_round_sustain: Duration::from_secs(5),
            notify_round_cooldown: Duration::from_secs(10),
            notify_ally_hp_sustain: Duration::from_secs(10),
        }
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
        }
    }
}

/// Serializable detection event for the frontend.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Serialize)]
pub struct DetectionEventPayload {
    /// Event kind (e.g., "`SkillGreyed`", "`RoundVisible`").
    pub kind: String,
    /// Human-readable detail.
    pub detail: String,
}

// --- Windows-only: monitor loop, TransitionFilter, and helpers ---
#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    /// Detector state category for transition tracking.
    ///
    /// Each detector produces two complementary event kinds (e.g., `SkillReady` /
    /// `SkillGreyed`). We track the last-seen kind per category and only forward
    /// events to the UI and notification manager when the kind changes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DetectorCategory {
        Skill,
        Round,
        Dialog,
        AllyHp,
        ResultScreen,
    }

    /// Tracks last-seen event kind per detector category, forwarding only transitions.
    ///
    /// Also suppresses Skill detector flapping during round transitions (the screen
    /// transition after `RoundGone` causes the skill icon to disappear temporarily).
    #[derive(Debug)]
    struct TransitionFilter {
        /// Maps each category to the last-seen event kind name.
        state: [Option<&'static str>; 5],
        /// When `RoundGone` was last seen (for skill suppression).
        round_gone_at: Option<Instant>,
        /// Duration to suppress skill events after round transition.
        round_transition_suppress: Duration,
    }

    impl TransitionFilter {
        const fn new(round_transition_suppress: Duration) -> Self {
            Self {
                state: [None; 5],
                round_gone_at: None,
                round_transition_suppress,
            }
        }

        /// Returns `true` if this event represents a state change worth showing.
        fn is_transition(&mut self, event: &DetectionEvent) -> bool {
            let now = Instant::now();

            // Track RoundGone timing
            if matches!(event, DetectionEvent::RoundGone { .. }) {
                self.round_gone_at = Some(now);
            }
            if matches!(event, DetectionEvent::RoundVisible { .. }) {
                self.round_gone_at = None;
            }

            // Suppress Skill flapping during round transition
            if matches!(
                event,
                DetectionEvent::SkillGreyed { .. }
                    | DetectionEvent::SkillReady { .. }
                    | DetectionEvent::SkillActive { .. }
                    | DetectionEvent::SkillOff { .. }
            ) && self
                .round_gone_at
                .is_some_and(|t| now.duration_since(t) < self.round_transition_suppress)
            {
                return false;
            }

            #[allow(clippy::as_conversions)] // enum to usize is safe
            let idx = categorize(event) as usize;
            let kind = event_kind_name(event);
            #[allow(clippy::indexing_slicing)] // idx is bounded by enum variant count
            let slot = &mut self.state[idx];
            if *slot == Some(kind) {
                return false;
            }
            *slot = Some(kind);
            true
        }

        /// Check if any event in the list would be a transition (without consuming).
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
            DetectionEvent::SkillReady { .. }
            | DetectionEvent::SkillActive { .. }
            | DetectionEvent::SkillOff { .. }
            | DetectionEvent::SkillGreyed { .. } => DetectorCategory::Skill,
            DetectionEvent::RoundVisible { .. } | DetectionEvent::RoundGone { .. } => {
                DetectorCategory::Round
            }
            DetectionEvent::DialogVisible { .. } | DetectionEvent::DialogGone { .. } => {
                DetectorCategory::Dialog
            }
            DetectionEvent::AllyHpLow { .. } | DetectionEvent::AllyHpNormal { .. } => {
                DetectorCategory::AllyHp
            }
            DetectionEvent::ResultScreenVisible { .. }
            | DetectionEvent::ResultScreenGone { .. } => DetectorCategory::ResultScreen,
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
        let mut transition_filter = TransitionFilter::new(monitor_config.round_transition_suppress);

        // Build detectors.
        // All detectors call analyze() directly. TransitionFilter handles
        // state-change deduplication, and round_transition_suppress handles
        // skill flapping during screen transitions.
        // AllyHpDetector is excluded (unverified placeholder config).
        let skill_detector = SkillDetector::new(det_config.skill);
        let round_detector = RoundDetector::new(det_config.round);
        let dialog_detector = DialogDetector::new(det_config.dialog);

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

            // Try WGC first, fall back to PrintWindow
            let (mut capturer, backend_name): (Box<dyn Capture>, &str) =
                match dna_capture::wgc::Capturer::start(hwnd) {
                    Ok(c) => {
                        info!("using WGC capture backend");
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
                        consecutive_failures = 0;
                        f
                    }
                    Err(e) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        warn!(%e, consecutive_failures, "capture failed");
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

                // Store latest frame for Detection page preview
                store_latest_frame(&latest_frame, frame.clone(), &capture_info);

                // Crop titlebar
                let game_frame = crop_titlebar(&frame);

                // Run pixel detectors
                let mut raw_events: Vec<DetectionEvent> = Vec::new();
                raw_events.extend(skill_detector.analyze(&game_frame));
                raw_events.extend(round_detector.analyze(&game_frame));
                raw_events.extend(dialog_detector.analyze(&game_frame));

                // OCR-assisted detection (Phase 2)
                // Only run OCR when pixel detector state changes (transition)
                // to avoid expensive OCR calls every frame.
                if let Some(ref ocr_engine) = ocr_engine
                    && transition_filter.has_pending_transition(&raw_events)
                {
                    run_ocr(ocr_engine, &game_frame, &mut raw_events);
                }

                // Notification uses raw events (needs sustained-condition tracking)
                if !raw_events.is_empty() {
                    notification_mgr.process_events(&raw_events);
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
                        let payload = DetectionEventPayload {
                            kind: event_kind_name(event).into(),
                            detail: event_description(event),
                        };
                        let _ = app_handle.emit("detection-event", &payload);
                    }
                }

                // Update frame timing + emit status once per frame
                let frame_ms = frame_start.elapsed().as_secs_f64().mul_add(1000.0, 0.0);
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
            DetectionEvent::SkillReady { .. } => "SkillReady",
            DetectionEvent::SkillActive { .. } => "SkillActive",
            DetectionEvent::SkillOff { .. } => "SkillOff",
            DetectionEvent::SkillGreyed { .. } => "SkillGreyed",
            DetectionEvent::AllyHpLow { .. } => "AllyHpLow",
            DetectionEvent::AllyHpNormal { .. } => "AllyHpNormal",
            DetectionEvent::RoundVisible { .. } => "RoundVisible",
            DetectionEvent::RoundGone { .. } => "RoundGone",
            DetectionEvent::ResultScreenVisible { .. } => "ResultScreenVisible",
            DetectionEvent::ResultScreenGone { .. } => "ResultScreenGone",
            DetectionEvent::DialogVisible { .. } => "DialogVisible",
            DetectionEvent::DialogGone { .. } => "DialogGone",
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

    /// Get a human-readable description for a detection event.
    fn event_description(event: &DetectionEvent) -> String {
        match event {
            DetectionEvent::SkillReady { .. } => String::from("Q スキル使用可能"),
            DetectionEvent::SkillActive { sp_cost, .. } => {
                format!("Q スキル発動中 (SP:{sp_cost})")
            }
            DetectionEvent::SkillOff { sp_cost, .. } => {
                format!("Q スキル未発動 (SP:{sp_cost})")
            }
            DetectionEvent::SkillGreyed { .. } => String::from("Q スキル SP 枯渇"),
            DetectionEvent::AllyHpLow { ally_index, .. } => {
                format!("味方 {ally_index} HP 低下")
            }
            DetectionEvent::AllyHpNormal { ally_index, .. } => {
                format!("味方 {ally_index} HP 回復")
            }
            DetectionEvent::RoundVisible {
                round_number: Some(n),
                ..
            } => {
                format!("ラウンド {n} 進行中")
            }
            DetectionEvent::RoundVisible { .. } => String::from("ラウンド進行中"),
            DetectionEvent::RoundGone { .. } => String::from("ラウンド完了"),
            DetectionEvent::ResultScreenVisible { .. } => String::from("依頼完了 (リザルト)"),
            DetectionEvent::ResultScreenGone { .. } => String::from("リザルト終了"),
            DetectionEvent::DialogVisible { .. } => String::from("ダイアログ表示"),
            DetectionEvent::DialogGone { .. } => String::from("ダイアログ消失"),
        }
    }

    /// Run OCR conditionally based on pixel detector results.
    ///
    /// - When `RoundVisible`: OCR the round text ROI to extract round number.
    /// - When `RoundGone`: OCR the result text area for "依頼完了" confirmation.
    /// - When `SkillReady`: OCR the skill area for SP cost to determine ON ("0") vs OFF.
    /// - When `DialogVisible`: OCR the dialog area for "Tips" title confirmation.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn run_ocr(
        ocr_engine: &dna_capture::ocr::JapaneseOcrEngine,
        game_frame: &image::RgbaImage,
        raw_events: &mut Vec<DetectionEvent>,
    ) {
        let has_skill_ready = raw_events
            .iter()
            .any(|e| matches!(e, DetectionEvent::SkillReady { .. }));
        let has_round_visible = raw_events
            .iter()
            .any(|e| matches!(e, DetectionEvent::RoundVisible { .. }));
        let has_round_gone = raw_events
            .iter()
            .any(|e| matches!(e, DetectionEvent::RoundGone { .. }));
        let has_dialog_visible = raw_events
            .iter()
            .any(|e| matches!(e, DetectionEvent::DialogVisible { .. }));

        // Enrich RoundVisible with OCR round number
        // Use an enlarged ROI (wider + taller than pixel detector ROI)
        // so Windows OCR has enough text height to recognize characters.
        let ocr_round_roi = dna_detector::roi::RoiDefinition {
            x: 0.0,
            y: 0.22,
            width: 0.30,
            height: 0.10,
        };
        if has_round_visible && let Some(ocr_image) = ocr_round_roi.crop(game_frame) {
            match ocr_engine.recognize_text(&ocr_image) {
                Ok(text) => {
                    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                    let has_round_text = normalized.contains("ラウンド");
                    let round_num = parse_round_number(&text);
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

        // Check for result screen when round text disappears
        // "依頼完了" appears in the bottom-left area of the result screen
        let ocr_result_roi = dna_detector::roi::RoiDefinition {
            x: 0.0,
            y: 0.75,
            width: 0.35,
            height: 0.20,
        };
        if has_round_gone && let Some(ocr_image) = ocr_result_roi.crop(game_frame) {
            match ocr_engine.recognize_text(&ocr_image) {
                Ok(text) => {
                    debug!(ocr_text = %text, "result screen OCR result");
                    if text.contains("依頼完了") {
                        info!(ocr_text = %text, "result screen detected via OCR");
                        raw_events.push(DetectionEvent::ResultScreenVisible {
                            text,
                            timestamp: Instant::now(),
                        });
                    }
                }
                Err(e) => {
                    debug!(%e, "result screen OCR failed");
                }
            }
        }

        // Skill ON/OFF via OCR: read SP cost number near the skill icon.
        // "0" = active (ON), any other number = inactive (OFF).
        // "Q" label and "強化" text are excluded from matching (gamepad hides Q).
        if has_skill_ready {
            let ocr_skill_roi = dna_detector::roi::RoiDefinition {
                x: 0.85,
                y: 0.86,
                width: 0.10,
                height: 0.10,
            };
            if let Some(roi_image) = ocr_skill_roi.crop(game_frame) {
                match ocr_engine.recognize_text(&roi_image) {
                    Ok(text) => {
                        // Parse SP cost: remove "Q" (key binding) and "強化" (label),
                        // extract remaining digits. "0" = active, non-zero = off.
                        let sp_cost = parse_sp_cost(&text);
                        debug!(sp_cost = ?sp_cost, ocr_text = %text, "skill OCR result");
                        if let Some(cost) = sp_cost {
                            let now = Instant::now();
                            if cost == "0" {
                                raw_events.push(DetectionEvent::SkillActive {
                                    sp_cost: cost,
                                    timestamp: now,
                                });
                            } else {
                                raw_events.push(DetectionEvent::SkillOff {
                                    sp_cost: cost,
                                    timestamp: now,
                                });
                            }
                        }
                        // sp_cost=None: OCR couldn't read SP number — skip
                    }
                    Err(e) => {
                        debug!(%e, "skill OCR failed");
                    }
                }
            }
        }

        // Gate DialogVisible via OCR: confirm "Tips" title is present.
        // Dark camera angles trigger false DialogVisible from pixel detection.
        // Dialog ROI covers the center area where "Tips" title appears
        let ocr_dialog_roi = dna_detector::roi::RoiDefinition {
            x: 0.25,
            y: 0.35,
            width: 0.50,
            height: 0.20,
        };
        if has_dialog_visible && let Some(ocr_image) = ocr_dialog_roi.crop(game_frame) {
            match ocr_engine.recognize_text(&ocr_image) {
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

    /// Extract SP cost number from OCR text near the skill icon.
    ///
    /// Ignores "Q" (gamepad hides it) and "強化" (always present).
    /// Returns the first numeric string found.
    fn parse_sp_cost(text: &str) -> Option<String> {
        let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();

        // Remove known non-cost text
        let cleaned = normalized.replace('Q', "").replace("強化", "");

        // Find first digit sequence
        let start = cleaned.find(|c: char| c.is_ascii_digit())?;
        let digits: String = cleaned[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            None
        } else {
            Some(digits)
        }
    }

    /// Parse round number from OCR text.
    ///
    /// Windows OCR often inserts spaces between characters, so we normalize
    /// by removing spaces before matching. Handles patterns like:
    /// - "探 検 現 在 の ラ ウ ン ド 20"
    /// - "探検 現在のラウンド：05"
    fn parse_round_number(text: &str) -> Option<u32> {
        // Remove spaces to normalize OCR output
        let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();

        // Look for digits after "ラウンド" marker
        let after = normalized.split("ラウンド").nth(1)?;

        // Skip optional colon/full-width colon
        let after = after
            .strip_prefix('：')
            .or_else(|| after.strip_prefix(':'))
            .unwrap_or(after);

        // Extract only the first consecutive digit sequence
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        digits.parse().ok()
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

        // Build filename: 20260329_173202_SkillGreyed_RoundGone.png
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
pub use platform::{CaptureInfo, MonitorState, start, stop};
