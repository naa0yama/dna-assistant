//! Notification manager with duplicate suppression and Discord webhook support.
//!
//! Converts detection events into Windows Toast or Discord notifications,
//! enforcing per-trigger cooldowns. Supports `RoundTrip` elapsed time thresholds
//! with Green/Yellow/Red alerting, and optional screenshot attachment for webhooks.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dna_detector::event::DetectionEvent;
use image::ImageFormat;
use tracing::{debug, instrument, warn};

use crate::monitor::{MonitorConfig, format_elapsed};

/// Maximum width for Discord screenshot attachment.
const DISCORD_IMAGE_MAX_WIDTH: u32 = 1920;
/// Maximum file size for Discord attachment (6 MB, safe for free tier 8 MB limit).
const DISCORD_IMAGE_MAX_BYTES: usize = 6 * 1024 * 1024;

/// Notification trigger kind, used as key for cooldown tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TriggerKind {
    DialogVisible,
    RoundGone,
    ResultScreen,
    RoundTripGreen,
    RoundTripYellow,
    RoundTripRed,
    CaptureLost,
    ResultIdle,
    GenemonVisible,
}

/// Configuration for a notification trigger.
struct TriggerConfig {
    /// How long the condition must persist before notifying.
    sustain_duration: Duration,
    /// Per-trigger cooldown between repeated notifications.
    cooldown: Duration,
    /// Notification title.
    title: &'static str,
    /// Notification body (may be overridden with dynamic text).
    body: &'static str,
}

/// Build trigger config from `MonitorConfig` values.
const fn trigger_config(kind: TriggerKind, cfg: &MonitorConfig) -> TriggerConfig {
    match kind {
        TriggerKind::DialogVisible => TriggerConfig {
            sustain_duration: cfg.notify_dialog_sustain,
            cooldown: cfg.notification_cooldown,
            title: "ダイアログ検出",
            body: "通信エラー等のダイアログが表示されています",
        },
        TriggerKind::RoundGone => TriggerConfig {
            sustain_duration: cfg.notify_round_sustain,
            cooldown: cfg.notify_round_cooldown,
            title: "ラウンド完了",
            body: "ラウンドが完了しました",
        },
        TriggerKind::ResultScreen => TriggerConfig {
            sustain_duration: Duration::from_secs(0),
            cooldown: cfg.notify_round_cooldown,
            title: "依頼完了",
            body: "ラウンドが完了しました (OCR 確認済み)",
        },
        TriggerKind::RoundTripGreen => TriggerConfig {
            sustain_duration: Duration::from_secs(0),
            cooldown: cfg.notify_round_cooldown,
            title: "RoundTrip: Green",
            body: "設定 Green より時間がかかっています",
        },
        TriggerKind::RoundTripYellow => TriggerConfig {
            sustain_duration: Duration::from_secs(0),
            cooldown: cfg.notify_round_cooldown,
            title: "RoundTrip: Yellow",
            body: "設定 Yellow より時間がかかっています",
        },
        TriggerKind::RoundTripRed => TriggerConfig {
            sustain_duration: Duration::from_secs(0),
            cooldown: cfg.notify_round_cooldown,
            title: "RoundTrip: Red",
            body: "設定 Red より時間がかかっています",
        },
        TriggerKind::CaptureLost => TriggerConfig {
            sustain_duration: Duration::from_secs(0),
            cooldown: cfg.notification_cooldown,
            title: "キャプチャ停止",
            body: "ウィンドウのキャプチャに失敗しました。最小化されていないか確認してください",
        },
        TriggerKind::ResultIdle => TriggerConfig {
            sustain_duration: Duration::from_secs(0),
            cooldown: cfg.notify_result_idle_threshold,
            title: "リザルト放置中",
            body: "リザルト画面が表示されたまま次のラウンドが始まっていません",
        },
        TriggerKind::GenemonVisible => TriggerConfig {
            sustain_duration: Duration::from_secs(0),
            cooldown: cfg.notification_cooldown,
            title: "ジェネモン発見",
            body: "ジェネモンを探して解放しよう",
        },
    }
}

/// Shared reference to the latest captured frame for screenshot attachment.
pub type SharedFrame = Arc<Mutex<crate::monitor::LatestFrame>>;

/// Build a reusable HTTP client for Discord webhook delivery.
///
/// Installs the global rustls crypto provider on first call (subsequent calls
/// are no-ops). A 30-second request timeout prevents indefinite blocking.
fn build_http_client() -> reqwest::blocking::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::blocking::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|e| {
            warn!(%e, "failed to build rustls HTTP client, falling back to default");
            reqwest::blocking::Client::new()
        })
}

/// Manages notification triggers with sustain-time and cooldown logic.
#[derive(Debug)]
pub struct NotificationManager {
    /// When each trigger condition first became active.
    condition_start: HashMap<TriggerKind, Instant>,
    /// When each trigger was last notified (for cooldown).
    last_notified: HashMap<TriggerKind, Instant>,
    /// True after `RoundVisible` is first seen. `RoundGone` notifications
    /// are suppressed until this becomes true.
    round_was_visible: bool,
    /// True when `RoundGone` has been notified and awaits `RoundVisible` reset.
    round_notified: bool,
    /// How many times the highest `RoundTrip` threshold has repeated.
    roundtrip_repeat_count: u32,
    /// How many times `CaptureLost` has repeated.
    capture_lost_repeat_count: u32,
    /// How many times `ResultIdle` has repeated.
    result_idle_repeat_count: u32,
    /// How many times `DialogVisible` has fired this occurrence.
    dialog_visible_repeat_count: u32,
    /// How many times `GenemonVisible` has fired this occurrence.
    genemon_visible_repeat_count: u32,
    /// Current round number (set externally from monitor loop).
    current_round: Option<u32>,
    /// Latest captured frame for Discord screenshot attachment.
    latest_frame: Option<SharedFrame>,
    /// Timing configuration.
    config: MonitorConfig,
    /// Reusable HTTP client for Discord webhook delivery.
    http_client: reqwest::blocking::Client,
}

impl NotificationManager {
    /// Create a new notification manager with the given configuration.
    pub fn new(config: &MonitorConfig) -> Self {
        Self {
            condition_start: HashMap::new(),
            last_notified: HashMap::new(),
            round_was_visible: false,
            round_notified: false,
            roundtrip_repeat_count: 0,
            capture_lost_repeat_count: 0,
            result_idle_repeat_count: 0,
            dialog_visible_repeat_count: 0,
            genemon_visible_repeat_count: 0,
            current_round: None,
            latest_frame: None,
            config: config.clone(),
            http_client: build_http_client(),
        }
    }

    /// Set the shared frame reference for Discord screenshot attachment.
    pub fn set_latest_frame(&mut self, frame: SharedFrame) {
        self.latest_frame = Some(frame);
    }

    /// Update the current round number for notification messages.
    pub const fn set_current_round(&mut self, round: Option<u32>) {
        self.current_round = round;
    }

    /// Notify `RoundTrip` threshold exceeded (called from monitor loop).
    ///
    /// Compares elapsed time against Green/Yellow/Red thresholds and sends
    /// the highest applicable notification.
    pub fn notify_roundtrip(&mut self, elapsed: Duration) {
        let now = Instant::now();

        let (kind, threshold_name) = if elapsed >= self.config.roundtrip_red {
            (TriggerKind::RoundTripRed, "Red")
        } else if elapsed >= self.config.roundtrip_yellow {
            (TriggerKind::RoundTripYellow, "Yellow")
        } else if elapsed >= self.config.roundtrip_green {
            (TriggerKind::RoundTripGreen, "Green")
        } else {
            return; // Below all thresholds
        };

        if !self.is_trigger_enabled(kind) {
            return;
        }

        let highest = self.highest_enabled_kind();
        if kind == highest {
            // Highest level: repeat at its own threshold interval.
            // e.g., Red=90s → fires at 90s, 180s, 270s...
            // Limited to max_repeat times.
            if self.roundtrip_repeat_count >= self.config.notification_max_repeat {
                return;
            }
            let threshold = self.threshold_for(kind);
            if let Some(&last) = self.last_notified.get(&kind)
                && now.duration_since(last) < threshold
            {
                return;
            }
        } else {
            // Lower levels: fire once only
            if self.last_notified.contains_key(&kind) {
                return;
            }
        }

        #[cfg(target_os = "windows")]
        if self.config.suppress_when_game_focused && Self::is_game_focused() {
            return;
        }

        let elapsed_str = format_elapsed(elapsed);
        let round_str = self
            .current_round
            .map_or_else(String::new, |r| format!("ラウンド {r:02} "));

        let body = format!(
            "{round_str}完了設定 {threshold_name} より時間がかかっています。(Elapsed={elapsed_str})"
        );
        let tc = trigger_config(kind, &self.config);
        let mention = matches!(
            kind,
            TriggerKind::RoundTripYellow | TriggerKind::RoundTripRed
        );

        self.send_notification_with_image(tc.title, &body, mention);
        self.last_notified.insert(kind, now);
        if kind == highest {
            self.roundtrip_repeat_count = self.roundtrip_repeat_count.saturating_add(1);
        }
    }

    /// Reset `RoundTrip` state (call on `RoundVisible` or `ResultScreenGone`).
    pub fn reset_roundtrip(&mut self) {
        self.roundtrip_repeat_count = 0;
        self.last_notified.remove(&TriggerKind::RoundTripGreen);
        self.last_notified.remove(&TriggerKind::RoundTripYellow);
        self.last_notified.remove(&TriggerKind::RoundTripRed);
    }

    /// Notify capture frame loss (called when consecutive failures exceed threshold).
    #[instrument(skip_all)]
    pub fn notify_capture_lost(&mut self) {
        let kind = TriggerKind::CaptureLost;

        if !self.is_trigger_enabled(kind) {
            debug!("capture lost notification suppressed: disabled");
            return;
        }

        if self.capture_lost_repeat_count >= self.config.notification_max_repeat {
            debug!("capture lost notification suppressed: max repeat reached");
            return;
        }

        let now = Instant::now();
        let tc = trigger_config(kind, &self.config);

        if let Some(&last) = self.last_notified.get(&kind)
            && now.duration_since(last) < tc.cooldown
        {
            debug!("capture lost notification suppressed: cooldown");
            return;
        }

        // Skip game-focus suppression: the game window is likely not visible.
        // Always send Windows toast (critical alert), plus Discord if enabled.
        self.send_toast_and_discord(tc.title, tc.body, true);
        self.last_notified.insert(kind, now);
        self.capture_lost_repeat_count = self.capture_lost_repeat_count.saturating_add(1);
    }

    /// Reset `CaptureLost` state and notify recovery if a lost notification was sent.
    #[instrument(skip_all)]
    pub fn reset_capture_lost(&mut self) {
        let was_notified = self.capture_lost_repeat_count > 0;
        self.capture_lost_repeat_count = 0;
        self.last_notified.remove(&TriggerKind::CaptureLost);

        if was_notified && self.is_trigger_enabled(TriggerKind::CaptureLost) {
            self.send_toast_and_discord(
                "キャプチャ復帰",
                "ウィンドウのキャプチャが復帰しました",
                false,
            );
        }
    }

    /// Notify result screen idle (called each tick while result is visible and no new round).
    ///
    /// Fires once after `elapsed >= notify_result_idle_threshold`, then repeats at the same
    /// interval up to `notification_max_repeat` times total.
    pub fn notify_result_idle(&mut self, elapsed: Duration) {
        let kind = TriggerKind::ResultIdle;

        if !self.config.notify_result_idle_enabled {
            return;
        }
        if elapsed < self.config.notify_result_idle_threshold {
            return;
        }
        if self.result_idle_repeat_count >= self.config.notification_max_repeat {
            return;
        }

        let now = Instant::now();
        let tc = trigger_config(kind, &self.config);
        if let Some(&last) = self.last_notified.get(&kind)
            && now.duration_since(last) < tc.cooldown
        {
            return;
        }

        #[cfg(target_os = "windows")]
        if self.config.suppress_when_game_focused && Self::is_game_focused() {
            return;
        }

        self.send_toast_and_discord(tc.title, tc.body, false);
        self.last_notified.insert(kind, now);
        self.result_idle_repeat_count = self.result_idle_repeat_count.saturating_add(1);
    }

    /// Reset `ResultIdle` state (call on `RoundVisible` or `ResultScreenGone`).
    pub fn reset_result_idle(&mut self) {
        self.result_idle_repeat_count = 0;
        self.last_notified.remove(&TriggerKind::ResultIdle);
    }

    /// Process detection events and send notifications if trigger conditions are met.
    #[instrument(skip_all)]
    pub fn process_events(&mut self, events: &[DetectionEvent]) {
        let now = Instant::now();

        for event in events {
            match event {
                DetectionEvent::RoundVisible { .. } => {
                    self.round_was_visible = true;
                    self.round_notified = false;
                    self.clear_condition(TriggerKind::RoundGone);
                }
                DetectionEvent::RoundGone { .. } => {
                    if self.round_was_visible && !self.round_notified {
                        self.track_condition(TriggerKind::RoundGone, now);
                    }
                }
                DetectionEvent::DialogVisible { .. } => {
                    self.track_condition(TriggerKind::DialogVisible, now);
                }
                DetectionEvent::DialogGone { .. } => {
                    self.clear_condition(TriggerKind::DialogVisible);
                }
                DetectionEvent::GenemonVisible { .. } => {
                    self.track_condition(TriggerKind::GenemonVisible, now);
                }
                DetectionEvent::GenemonGone { .. } => {
                    self.clear_condition(TriggerKind::GenemonVisible);
                }
                // ResultScreen: handled via confirmed transitions, not raw events.
                // RoundSelectScreen: internal-only, no notifications.
                DetectionEvent::ResultScreenVisible { .. }
                | DetectionEvent::ResultScreenGone { .. }
                | DetectionEvent::RoundSelectScreen { .. } => {}
            }
        }

        // Check all active conditions for sustained triggers
        let active_kinds: Vec<TriggerKind> = self.condition_start.keys().copied().collect();
        for kind in active_kinds {
            self.check_and_notify(kind, now);
        }
    }

    /// Start tracking a condition (or keep existing start time).
    fn track_condition(&mut self, kind: TriggerKind, now: Instant) {
        self.condition_start.entry(kind).or_insert(now);
    }

    /// Clear a condition when the opposite event is received.
    ///
    /// Resets the repeat counter and last-notified time for dialog/genemon so
    /// a fresh occurrence after dismissal is not blocked by the previous cooldown.
    fn clear_condition(&mut self, kind: TriggerKind) {
        self.condition_start.remove(&kind);
        if matches!(
            kind,
            TriggerKind::DialogVisible | TriggerKind::GenemonVisible
        ) {
            self.reset_repeat_count(kind);
            self.last_notified.remove(&kind);
        }
    }

    /// Check if a specific trigger kind is enabled via config toggles.
    const fn is_trigger_enabled(&self, kind: TriggerKind) -> bool {
        if !self.config.notifications_enabled {
            return false;
        }
        match kind {
            TriggerKind::RoundGone => self.config.notify_round_enabled,
            TriggerKind::DialogVisible => self.config.notify_dialog_enabled,
            TriggerKind::ResultScreen => self.config.notify_result_enabled,
            TriggerKind::RoundTripGreen => self.config.notify_roundtrip_green,
            TriggerKind::RoundTripYellow => self.config.notify_roundtrip_yellow,
            TriggerKind::RoundTripRed => self.config.notify_roundtrip_red,
            TriggerKind::CaptureLost => self.config.notify_capture_lost_enabled,
            TriggerKind::ResultIdle => self.config.notify_result_idle_enabled,
            TriggerKind::GenemonVisible => self.config.notify_genemon_enabled,
        }
    }

    /// Check if the game window is currently the foreground window.
    #[cfg(target_os = "windows")]
    fn is_game_focused() -> bool {
        dna_capture::window::is_game_foreground()
    }

    /// Notify `ResultScreen` detection (called after `TransitionFilter` confirmation).
    #[instrument(skip_all)]
    pub fn notify_result_screen(&mut self) {
        let kind = TriggerKind::ResultScreen;

        if !self.is_trigger_enabled(kind) {
            debug!("result screen notification suppressed: disabled");
            return;
        }

        let now = Instant::now();
        let tc = trigger_config(kind, &self.config);

        // Check cooldown
        if let Some(&last) = self.last_notified.get(&kind)
            && now.duration_since(last) < tc.cooldown
        {
            debug!("result screen notification suppressed: cooldown");
            return;
        }

        #[cfg(target_os = "windows")]
        if self.config.suppress_when_game_focused && Self::is_game_focused() {
            debug!("result screen notification suppressed: game focused");
            return;
        }

        let body = self.current_round.map_or_else(
            || String::from(tc.body),
            |round| format!("ラウンド {round:02} が完了しました (OCR 確認済み)"),
        );

        let mention = true;
        self.send_notification_with_image(tc.title, &body, mention);
        self.last_notified.insert(kind, now);
    }

    /// Return the highest enabled `RoundTrip` trigger kind.
    const fn highest_enabled_kind(&self) -> TriggerKind {
        if self.config.notify_roundtrip_red {
            TriggerKind::RoundTripRed
        } else if self.config.notify_roundtrip_yellow {
            TriggerKind::RoundTripYellow
        } else {
            TriggerKind::RoundTripGreen
        }
    }

    /// Return the threshold duration for a `RoundTrip` kind.
    const fn threshold_for(&self, kind: TriggerKind) -> Duration {
        match kind {
            TriggerKind::RoundTripRed => self.config.roundtrip_red,
            TriggerKind::RoundTripYellow => self.config.roundtrip_yellow,
            // Green or any other kind (only called from notify_roundtrip).
            _ => self.config.roundtrip_green,
        }
    }

    /// Return the current repeat count for a condition-tracked trigger kind.
    const fn repeat_count_for(&self, kind: TriggerKind) -> u32 {
        match kind {
            TriggerKind::DialogVisible => self.dialog_visible_repeat_count,
            TriggerKind::GenemonVisible => self.genemon_visible_repeat_count,
            _ => 0,
        }
    }

    /// Increment the repeat counter for a condition-tracked trigger kind.
    fn increment_repeat_count(&mut self, kind: TriggerKind) {
        match kind {
            TriggerKind::DialogVisible => {
                self.dialog_visible_repeat_count =
                    self.dialog_visible_repeat_count.saturating_add(1);
            }
            TriggerKind::GenemonVisible => {
                self.genemon_visible_repeat_count =
                    self.genemon_visible_repeat_count.saturating_add(1);
            }
            _ => {}
        }
    }

    /// Reset the repeat counter for a condition-tracked trigger kind.
    fn reset_repeat_count(&mut self, kind: TriggerKind) {
        match kind {
            TriggerKind::DialogVisible => self.dialog_visible_repeat_count = 0,
            TriggerKind::GenemonVisible => self.genemon_visible_repeat_count = 0,
            _ => {}
        }
    }

    /// Check if a condition has been sustained long enough and send notification.
    fn check_and_notify(&mut self, kind: TriggerKind, now: Instant) {
        let Some(&start) = self.condition_start.get(&kind) else {
            return;
        };

        if !self.is_trigger_enabled(kind) {
            return;
        }

        // Enforce max_repeat for condition-tracked triggers that support it.
        if matches!(
            kind,
            TriggerKind::DialogVisible | TriggerKind::GenemonVisible
        ) && self.repeat_count_for(kind) >= self.config.notification_max_repeat
        {
            debug!(
                ?kind,
                max_repeat = self.config.notification_max_repeat,
                "condition notification suppressed: max repeat reached"
            );
            return;
        }

        let tc = trigger_config(kind, &self.config);

        if now.duration_since(start) < tc.sustain_duration {
            return;
        }

        if let Some(&last) = self.last_notified.get(&kind)
            && now.duration_since(last) < tc.cooldown
        {
            return;
        }

        #[cfg(target_os = "windows")]
        if self.config.suppress_when_game_focused && Self::is_game_focused() {
            return;
        }

        // Build notification text (include round number if available)
        let body = self.current_round.map_or_else(
            || String::from(tc.body),
            |round| match kind {
                TriggerKind::RoundGone => format!("ラウンド {round:02} が完了しました"),
                TriggerKind::ResultScreen => {
                    format!("ラウンド {round:02} が完了しました (OCR 確認済み)")
                }
                _ => String::from(tc.body),
            },
        );

        let mention = matches!(kind, TriggerKind::DialogVisible | TriggerKind::ResultScreen);
        self.send_notification_with_image(tc.title, &body, mention);
        self.last_notified.insert(kind, now);
        self.increment_repeat_count(kind);
        self.condition_start.remove(&kind);

        if kind == TriggerKind::RoundGone {
            self.round_notified = true;
            self.round_was_visible = false;
        }
    }

    /// Send notification with optional screenshot (Discord) and/or toast (desktop).
    fn send_notification_with_image(&self, title: &str, body: &str, mention: bool) {
        if self.config.is_discord_active() {
            let image_data = self.capture_screenshot();
            let mention_id = if mention {
                Some(self.config.discord_mention_id.clone())
            } else {
                None
            };
            let client = self.http_client.clone();
            let webhook_url = self.config.discord_webhook_url.clone();
            let title_d = title.to_owned();
            let body_d = body.to_owned();
            std::thread::spawn(move || {
                Self::send_discord(
                    &client,
                    &webhook_url,
                    &title_d,
                    &body_d,
                    image_data,
                    mention_id.as_deref(),
                );
            });
        }
        if self.config.desktop_enabled {
            Self::send_toast(title, body);
        }
    }

    /// Send Windows toast (when enabled) AND Discord webhook (when configured).
    fn send_toast_and_discord(&self, title: &str, body: &str, mention: bool) {
        if self.config.desktop_enabled {
            Self::send_toast(title, body);
        }
        if self.config.is_discord_active() {
            let mention_id = if mention {
                Some(self.config.discord_mention_id.clone())
            } else {
                None
            };
            let client = self.http_client.clone();
            let webhook_url = self.config.discord_webhook_url.clone();
            let title = title.to_owned();
            let body = body.to_owned();
            std::thread::spawn(move || {
                Self::send_discord(
                    &client,
                    &webhook_url,
                    &title,
                    &body,
                    None,
                    mention_id.as_deref(),
                );
            });
        }
    }

    /// Capture the latest frame as PNG bytes for Discord attachment.
    fn capture_screenshot(&self) -> Option<Vec<u8>> {
        let frame_ref = self.latest_frame.as_ref()?;
        let image_arc = {
            let guard = frame_ref.lock().ok()?;
            guard.image.clone()?
        };

        // Downscale if wider than FHD
        let img = if image_arc.width() > DISCORD_IMAGE_MAX_WIDTH {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            let new_h = (f64::from(image_arc.height()) * f64::from(DISCORD_IMAGE_MAX_WIDTH)
                / f64::from(image_arc.width())) as u32;
            image::DynamicImage::from(image::imageops::resize(
                image_arc.as_ref(),
                DISCORD_IMAGE_MAX_WIDTH,
                new_h,
                image::imageops::FilterType::Triangle,
            ))
        } else {
            image::DynamicImage::ImageRgba8(image_arc.as_ref().clone())
        };

        let mut buf = Cursor::new(Vec::new());
        if img.write_to(&mut buf, ImageFormat::Png).is_err() {
            return None;
        }

        let png_bytes = buf.into_inner();

        // Check file size limit
        if png_bytes.len() > DISCORD_IMAGE_MAX_BYTES {
            debug!(
                size = png_bytes.len(),
                "screenshot exceeds Discord size limit, skipping"
            );
            return None;
        }

        Some(png_bytes)
    }

    /// Send a test notification to verify delivery (Discord and/or toast).
    pub fn send_test_notification(config: &MonitorConfig) {
        let title = "DNA Assistant テスト";
        let body = "通知が正常に動作しています";
        if config.is_discord_active() {
            let mention_id = if config.discord_mention_id.is_empty() {
                None
            } else {
                Some(config.discord_mention_id.clone())
            };
            let client = build_http_client();
            let webhook_url = config.discord_webhook_url.clone();
            std::thread::spawn(move || {
                Self::send_discord(
                    &client,
                    &webhook_url,
                    title,
                    body,
                    None,
                    mention_id.as_deref(),
                );
            });
        }
        if config.desktop_enabled {
            Self::send_toast(title, body);
        }
    }

    /// Send a notification via Discord webhook with optional image and mention.
    fn send_discord(
        client: &reqwest::blocking::Client,
        webhook_url: &str,
        title: &str,
        body: &str,
        image: Option<Vec<u8>>,
        mention_id: Option<&str>,
    ) {
        debug!(
            title,
            body,
            has_image = image.is_some(),
            "sending Discord webhook"
        );

        // Build text content: title (+ mention if configured).
        // Discord notification preview only shows `content`, not embed fields.
        let content = mention_id.filter(|id| !id.is_empty()).map_or_else(
            || format!("**{title}**"),
            |id| format!("<@{id}> **{title}**"),
        );

        #[allow(clippy::option_if_let_else)] // Complex multipart vs json branches
        let result = if let Some(png_bytes) = image {
            let payload_json = serde_json::json!({
                "content": content,
                "embeds": [{
                    "description": body,
                    "color": 5_814_783,
                    "image": { "url": "attachment://capture.png" }
                }]
            });

            let Ok(payload_str) = serde_json::to_string(&payload_json).inspect_err(|e| {
                warn!(%e, "failed to serialize Discord multipart payload, skipping");
            }) else {
                return;
            };

            let form = reqwest::blocking::multipart::Form::new()
                .text("payload_json", payload_str)
                .part(
                    "files[0]",
                    reqwest::blocking::multipart::Part::bytes(png_bytes)
                        .file_name("capture.png")
                        .mime_str("image/png")
                        .unwrap_or_else(|_| reqwest::blocking::multipart::Part::bytes(Vec::new())),
                );

            client.post(webhook_url).multipart(form).send()
        } else {
            let payload = serde_json::json!({
                "content": content,
                "embeds": [{
                    "description": body,
                    "color": 5_814_783
                }]
            });
            client.post(webhook_url).json(&payload).send()
        };

        match result {
            Ok(resp) if !resp.status().is_success() => {
                warn!(status = %resp.status(), "Discord webhook returned non-success status");
            }
            Ok(_) => {
                debug!("Discord webhook sent successfully");
            }
            Err(e) => {
                warn!(%e, "failed to send Discord webhook");
            }
        }
    }

    /// Check if the app is running from an installed location (not `cargo run`).
    #[cfg(target_os = "windows")]
    fn is_installed_app() -> bool {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .is_some_and(|path| {
                !path.contains("target\\debug") && !path.contains("target\\release")
            })
    }

    /// Test accessor: repeat count for `DialogVisible`.
    #[cfg(test)]
    pub(crate) fn dialog_repeat_count(&self) -> u32 {
        self.dialog_visible_repeat_count
    }

    /// Test accessor: repeat count for `GenemonVisible`.
    #[cfg(test)]
    pub(crate) fn genemon_repeat_count(&self) -> u32 {
        self.genemon_visible_repeat_count
    }

    /// Test accessor: whether a trigger was last-notified.
    #[cfg(test)]
    pub(crate) fn was_notified(&self, kind: TriggerKind) -> bool {
        self.last_notified.contains_key(&kind)
    }

    /// Test accessor: whether a condition is being tracked.
    #[cfg(test)]
    pub(crate) fn is_tracking(&self, kind: TriggerKind) -> bool {
        self.condition_start.contains_key(&kind)
    }

    /// Test accessor: repeat count for `CaptureLost`.
    #[cfg(test)]
    pub(crate) fn capture_lost_count(&self) -> u32 {
        self.capture_lost_repeat_count
    }

    /// Test accessor: repeat count for `ResultIdle`.
    #[cfg(test)]
    pub(crate) fn result_idle_count(&self) -> u32 {
        self.result_idle_repeat_count
    }

    fn send_toast(title: &str, body: &str) {
        debug!(title, body, "sending toast notification");

        let mut notification = notify_rust::Notification::new();
        notification.summary(title).body(body);

        #[cfg(target_os = "windows")]
        {
            let app_id = if Self::is_installed_app() {
                "com.naa0yama.dna-assistant"
            } else {
                "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe"
            };
            notification
                .app_id(app_id)
                .sound_name("Default")
                .timeout(notify_rust::Timeout::Milliseconds(25_000));
        }

        let result = notification.show();

        if let Err(e) = result {
            warn!(%e, "failed to send toast notification");
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::time::Duration;

    use dna_detector::event::DetectionEvent;

    use super::*;
    use crate::monitor::MonitorConfig;

    fn test_config_zero() -> MonitorConfig {
        MonitorConfig {
            notify_dialog_sustain: Duration::ZERO,
            notify_round_sustain: Duration::ZERO,
            notify_dialog_enabled: true,
            notify_round_enabled: true,
            notify_genemon_enabled: true,
            notify_capture_lost_enabled: true,
            notify_result_idle_enabled: true,
            notifications_enabled: true,
            notification_max_repeat: 3,
            notification_cooldown: Duration::ZERO,
            notify_round_cooldown: Duration::ZERO,
            desktop_enabled: false,
            discord_enabled: false,
            ..Default::default()
        }
    }

    fn test_config_with_cooldown(cooldown: Duration) -> MonitorConfig {
        MonitorConfig {
            notification_cooldown: cooldown,
            ..test_config_zero()
        }
    }

    fn new_mgr(cfg: &MonitorConfig) -> NotificationManager {
        NotificationManager::new(cfg)
    }

    fn dialog_visible() -> DetectionEvent {
        DetectionEvent::DialogVisible {
            text_ratio: 0.5,
            bg_dark_ratio: 0.5,
            timestamp: std::time::Instant::now(),
        }
    }

    fn dialog_gone() -> DetectionEvent {
        DetectionEvent::DialogGone {
            text_ratio: 0.0,
            bg_dark_ratio: 0.0,
            timestamp: std::time::Instant::now(),
        }
    }

    fn genemon_visible() -> DetectionEvent {
        DetectionEvent::GenemonVisible {
            timestamp: std::time::Instant::now(),
        }
    }

    fn genemon_gone() -> DetectionEvent {
        DetectionEvent::GenemonGone {
            timestamp: std::time::Instant::now(),
        }
    }

    fn round_visible() -> DetectionEvent {
        DetectionEvent::RoundVisible {
            text_present: true,
            white_ratio: 0.5,
            round_number: Some(1),
            timestamp: std::time::Instant::now(),
        }
    }

    fn round_gone() -> DetectionEvent {
        DetectionEvent::RoundGone {
            white_ratio: 0.0,
            timestamp: std::time::Instant::now(),
        }
    }

    // ── DialogVisible lifecycle ──────────────────────────────────────────────

    #[test]
    fn dialog_fires_up_to_max_repeat() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        for _ in 0..cfg.notification_max_repeat {
            mgr.process_events(&[dialog_visible()]);
        }
        assert_eq!(mgr.dialog_repeat_count(), cfg.notification_max_repeat);
    }

    #[test]
    fn dialog_stops_after_max_repeat() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        for _ in 0..=cfg.notification_max_repeat {
            mgr.process_events(&[dialog_visible()]);
        }
        assert_eq!(mgr.dialog_repeat_count(), cfg.notification_max_repeat);
    }

    #[test]
    fn dialog_resets_on_gone() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        for _ in 0..cfg.notification_max_repeat {
            mgr.process_events(&[dialog_visible()]);
        }
        mgr.process_events(&[dialog_gone()]);
        assert_eq!(mgr.dialog_repeat_count(), 0);
        // After reset, fires again
        mgr.process_events(&[dialog_visible()]);
        assert_eq!(mgr.dialog_repeat_count(), 1);
    }

    #[test]
    fn dialog_cooldown_suppresses_repeat() {
        let cfg = test_config_with_cooldown(Duration::from_secs(3600));
        let mut mgr = new_mgr(&cfg);
        mgr.process_events(&[dialog_visible()]);
        assert_eq!(mgr.dialog_repeat_count(), 1);
        // Second call within cooldown must not fire again
        mgr.process_events(&[dialog_visible()]);
        assert_eq!(mgr.dialog_repeat_count(), 1);
    }

    #[test]
    fn dialog_sustain_blocks_premature_fire() {
        let cfg = MonitorConfig {
            notify_dialog_sustain: Duration::from_secs(3600),
            ..test_config_zero()
        };
        let mut mgr = new_mgr(&cfg);
        mgr.process_events(&[dialog_visible()]);
        assert!(!mgr.was_notified(TriggerKind::DialogVisible));
    }

    // ── GenemonVisible lifecycle ─────────────────────────────────────────────

    #[test]
    fn genemon_fires_up_to_max_repeat() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        for _ in 0..cfg.notification_max_repeat {
            mgr.process_events(&[genemon_visible()]);
        }
        assert_eq!(mgr.genemon_repeat_count(), cfg.notification_max_repeat);
    }

    #[test]
    fn genemon_resets_on_gone() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        for _ in 0..cfg.notification_max_repeat {
            mgr.process_events(&[genemon_visible()]);
        }
        mgr.process_events(&[genemon_gone()]);
        assert_eq!(mgr.genemon_repeat_count(), 0);
        mgr.process_events(&[genemon_visible()]);
        assert_eq!(mgr.genemon_repeat_count(), 1);
    }

    // ── RoundGone lifecycle ──────────────────────────────────────────────────

    #[test]
    fn round_gone_suppressed_before_round_visible() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        mgr.process_events(&[round_gone()]);
        assert!(!mgr.was_notified(TriggerKind::RoundGone));
    }

    #[test]
    fn round_gone_fires_after_round_visible() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        mgr.process_events(&[round_visible()]);
        mgr.process_events(&[round_gone()]);
        assert!(mgr.was_notified(TriggerKind::RoundGone));
    }

    #[test]
    fn round_gone_fires_once_per_round() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        mgr.process_events(&[round_visible()]);
        mgr.process_events(&[round_gone()]);
        assert!(mgr.was_notified(TriggerKind::RoundGone));
        // Second RoundGone: already notified for this round, must not re-fire
        mgr.process_events(&[round_gone()]);
        // round_notified stays true, was_notified key should still be present
        assert!(mgr.was_notified(TriggerKind::RoundGone));
    }

    #[test]
    fn round_visible_resets_for_next_round() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        mgr.process_events(&[round_visible()]);
        mgr.process_events(&[round_gone()]);
        // New round starts
        mgr.process_events(&[round_visible()]);
        mgr.process_events(&[round_gone()]);
        assert!(mgr.was_notified(TriggerKind::RoundGone));
    }

    // ── CaptureLost ──────────────────────────────────────────────────────────

    #[test]
    fn capture_lost_max_repeat() {
        let cfg = test_config_zero();
        let mut mgr = new_mgr(&cfg);
        for _ in 0..=cfg.notification_max_repeat {
            mgr.notify_capture_lost();
        }
        assert_eq!(mgr.capture_lost_count(), cfg.notification_max_repeat);
        mgr.reset_capture_lost();
        assert_eq!(mgr.capture_lost_count(), 0);
    }

    // ── ResultIdle ───────────────────────────────────────────────────────────

    #[test]
    fn result_idle_threshold_and_max_repeat() {
        let cfg = MonitorConfig {
            notify_result_idle_threshold: Duration::from_secs(5),
            ..test_config_zero()
        };
        let mut mgr = new_mgr(&cfg);
        // Below threshold: no fire
        mgr.notify_result_idle(Duration::from_secs(4));
        assert_eq!(mgr.result_idle_count(), 0);
        // At threshold: fires
        mgr.notify_result_idle(Duration::from_secs(5));
        assert_eq!(mgr.result_idle_count(), 1);
        // Exceeds max_repeat: stops
        for _ in 0..cfg.notification_max_repeat {
            mgr.notify_result_idle(Duration::from_secs(5));
        }
        assert_eq!(mgr.result_idle_count(), cfg.notification_max_repeat);
    }
}
