//! Background monitor loop: capture → detect → notify.
//!
//! Runs on a dedicated thread, capturing game frames at regular intervals,
//! feeding them through the detection pipeline, and sending notifications
//! when trigger conditions are met.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use dna_capture::Capture;
use dna_detector::config::DetectionConfig;
use dna_detector::detector::Detector;
use dna_detector::detector::dialog::DialogDetector;
use dna_detector::detector::round::RoundDetector;
use dna_detector::detector::skill::SkillDetector;
use dna_detector::event::DetectionEvent;
use dna_detector::state::DebouncedDetector;
use dna_detector::titlebar::crop_titlebar;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tracing::{debug, error, info, instrument, trace, warn};

use crate::notification::NotificationManager;

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
    /// Debounce cooldown for skill detector (ms).
    #[serde(with = "serde_duration_ms")]
    pub skill_debounce: Duration,
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

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            capture_interval: Duration::from_millis(2000),
            window_search_interval: Duration::from_millis(3000),
            max_capture_retries: 3,
            preview_interval: Duration::from_millis(3000),
            skill_debounce: Duration::from_millis(2500),
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

/// Granularity for interruptible sleeps (check stop flag every 200ms).
const SLEEP_GRANULARITY: Duration = Duration::from_millis(200);

/// Current monitoring state.
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
#[derive(Debug, Clone, Serialize)]
pub struct DetectionEventPayload {
    /// Event kind (e.g., "`SkillGreyed`", "`RoundVisible`").
    pub kind: String,
    /// Human-readable detail.
    pub detail: String,
}

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
}

/// Tracks last-seen event kind per detector category, forwarding only transitions.
///
/// Also suppresses Skill detector flapping during round transitions (the screen
/// transition after `RoundGone` causes the skill icon to disappear temporarily).
#[derive(Debug)]
struct TransitionFilter {
    /// Maps each category to the last-seen event kind name.
    state: [Option<&'static str>; 4],
    /// When `RoundGone` was last seen (for skill suppression).
    round_gone_at: Option<Instant>,
    /// Duration to suppress skill events after round transition.
    round_transition_suppress: Duration,
}

impl TransitionFilter {
    const fn new(round_transition_suppress: Duration) -> Self {
        Self {
            state: [None; 4],
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
            DetectionEvent::SkillGreyed { .. } | DetectionEvent::SkillReady { .. }
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
}

/// Map a detection event to its detector category.
const fn categorize(event: &DetectionEvent) -> DetectorCategory {
    match event {
        DetectionEvent::SkillReady { .. } | DetectionEvent::SkillGreyed { .. } => {
            DetectorCategory::Skill
        }
        DetectionEvent::RoundVisible { .. } | DetectionEvent::RoundGone { .. } => {
            DetectorCategory::Round
        }
        DetectionEvent::DialogVisible { .. } | DetectionEvent::DialogGone { .. } => {
            DetectorCategory::Dialog
        }
        DetectionEvent::AllyHpLow { .. } | DetectionEvent::AllyHpNormal { .. } => {
            DetectorCategory::AllyHp
        }
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
    pub handle: Mutex<Option<MonitorHandle>>,
    pub status: Arc<Mutex<MonitorStatus>>,
    pub latest_frame: Arc<Mutex<LatestFrame>>,
    pub config: Arc<Mutex<MonitorConfig>>,
}

impl MonitorState {
    /// Create with default config (call `load_config` after `AppHandle` is available).
    #[must_use]
    pub fn new() -> Self {
        Self {
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
    // SkillDetector uses DebouncedDetector to suppress cut-in animation false positives.
    // RoundDetector and DialogDetector call analyze() directly — TransitionFilter
    // handles state-change filtering for the UI.
    // AllyHpDetector is excluded (unverified placeholder config, always fires AllyHpLow).
    let mut skill_detector = DebouncedDetector::new(
        SkillDetector::new(det_config.skill),
        monitor_config.skill_debounce,
    );
    let round_detector = RoundDetector::new(det_config.round);
    let dialog_detector = DialogDetector::new(det_config.dialog);

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

            // Run detectors
            let mut raw_events: Vec<DetectionEvent> = Vec::new();
            raw_events.extend(skill_detector.process(&game_frame));
            raw_events.extend(round_detector.analyze(&game_frame));
            raw_events.extend(dialog_detector.analyze(&game_frame));

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
        DetectionEvent::SkillGreyed { .. } => "SkillGreyed",
        DetectionEvent::AllyHpLow { .. } => "AllyHpLow",
        DetectionEvent::AllyHpNormal { .. } => "AllyHpNormal",
        DetectionEvent::RoundVisible { .. } => "RoundVisible",
        DetectionEvent::RoundGone { .. } => "RoundGone",
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
        DetectionEvent::SkillGreyed { .. } => String::from("Q スキル SP 枯渇"),
        DetectionEvent::AllyHpLow { ally_index, .. } => {
            format!("味方 {ally_index} HP 低下")
        }
        DetectionEvent::AllyHpNormal { ally_index, .. } => {
            format!("味方 {ally_index} HP 回復")
        }
        DetectionEvent::RoundVisible { .. } => String::from("ラウンド進行中"),
        DetectionEvent::RoundGone { .. } => String::from("ラウンド完了"),
        DetectionEvent::DialogVisible { .. } => String::from("ダイアログ表示"),
        DetectionEvent::DialogGone { .. } => String::from("ダイアログ消失"),
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
