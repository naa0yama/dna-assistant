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

use crate::monitor::MonitorConfig;

/// Maximum width for Discord screenshot attachment.
const DISCORD_IMAGE_MAX_WIDTH: u32 = 1920;
/// Maximum file size for Discord attachment (6 MB, safe for free tier 8 MB limit).
const DISCORD_IMAGE_MAX_BYTES: usize = 6 * 1024 * 1024;

/// Notification trigger kind, used as key for cooldown tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TriggerKind {
    DialogVisible,
    RoundGone,
    ResultScreen,
    RoundTripGreen,
    RoundTripYellow,
    RoundTripRed,
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
    }
}

/// Shared reference to the latest captured frame for screenshot attachment.
pub type SharedFrame = Arc<Mutex<crate::monitor::LatestFrame>>;

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
    /// Current round number (set externally from monitor loop).
    current_round: Option<u32>,
    /// Latest captured frame for Discord screenshot attachment.
    latest_frame: Option<SharedFrame>,
    /// Timing configuration.
    config: MonitorConfig,
}

impl NotificationManager {
    /// Create a new notification manager with the given configuration.
    pub fn new(config: &MonitorConfig) -> Self {
        Self {
            condition_start: HashMap::new(),
            last_notified: HashMap::new(),
            round_was_visible: false,
            round_notified: false,
            current_round: None,
            latest_frame: None,
            config: config.clone(),
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

        // Check cooldown
        if let Some(&last) = self.last_notified.get(&kind)
            && now.duration_since(last) < self.config.notify_round_cooldown
        {
            return;
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
                DetectionEvent::ResultScreenVisible { .. } => {
                    self.track_condition(TriggerKind::ResultScreen, now);
                }
                DetectionEvent::ResultScreenGone { .. } => {
                    self.clear_condition(TriggerKind::ResultScreen);
                }
                // Events that don't trigger notifications
                _ => {}
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
    fn clear_condition(&mut self, kind: TriggerKind) {
        self.condition_start.remove(&kind);
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
        }
    }

    /// Check if the game window is currently the foreground window.
    #[cfg(target_os = "windows")]
    fn is_game_focused() -> bool {
        dna_capture::window::is_game_foreground()
    }

    /// Check if a condition has been sustained long enough and send notification.
    fn check_and_notify(&mut self, kind: TriggerKind, now: Instant) {
        let Some(&start) = self.condition_start.get(&kind) else {
            return;
        };

        if !self.is_trigger_enabled(kind) {
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

        let mention = matches!(kind, TriggerKind::DialogVisible);
        self.send_notification_with_image(tc.title, &body, mention);
        self.last_notified.insert(kind, now);
        self.condition_start.remove(&kind);

        if kind == TriggerKind::RoundGone {
            self.round_notified = true;
            self.round_was_visible = false;
        }
    }

    /// Send notification with optional screenshot and mention (Discord only).
    fn send_notification_with_image(&self, title: &str, body: &str, mention: bool) {
        if self.config.discord_enabled && !self.config.discord_webhook_url.is_empty() {
            let image_data = self.capture_screenshot();
            let mention_id = if mention {
                Some(self.config.discord_mention_id.as_str())
            } else {
                None
            };
            Self::send_discord(
                &self.config.discord_webhook_url,
                title,
                body,
                image_data,
                mention_id,
            );
        } else {
            Self::send_toast(title, body);
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

    /// Send a test notification to verify delivery (Discord or toast).
    pub fn send_test_notification(config: &MonitorConfig) {
        let title = "DNA Assistant テスト";
        let body = "通知が正常に動作しています";
        if config.discord_enabled && !config.discord_webhook_url.is_empty() {
            let mention_id = if config.discord_mention_id.is_empty() {
                None
            } else {
                Some(config.discord_mention_id.as_str())
            };
            Self::send_discord(&config.discord_webhook_url, title, body, None, mention_id);
        } else {
            Self::send_toast(title, body);
        }
    }

    /// Send a notification via Discord webhook with optional image and mention.
    fn send_discord(
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

        let _ = rustls::crypto::ring::default_provider().install_default();

        let client = match reqwest::blocking::Client::builder()
            .use_rustls_tls()
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!(%e, "failed to create HTTP client for Discord webhook");
                return;
            }
        };

        // Build mention content if ID is provided and non-empty
        let mention_content = mention_id
            .filter(|id| !id.is_empty())
            .map(|id| format!("<@{id}>"));

        #[allow(clippy::option_if_let_else)] // Complex multipart vs json branches
        let result = if let Some(png_bytes) = image {
            let mut payload_json = serde_json::json!({
                "embeds": [{
                    "title": title,
                    "description": body,
                    "color": 5_814_783,
                    "image": { "url": "attachment://capture.png" }
                }]
            });
            if let Some(ref mention) = mention_content
                && let Some(obj) = payload_json.as_object_mut()
            {
                obj.insert("content".into(), serde_json::json!(mention));
            }

            let form = reqwest::blocking::multipart::Form::new()
                .text(
                    "payload_json",
                    serde_json::to_string(&payload_json).unwrap_or_default(),
                )
                .part(
                    "files[0]",
                    reqwest::blocking::multipart::Part::bytes(png_bytes)
                        .file_name("capture.png")
                        .mime_str("image/png")
                        .unwrap_or_else(|_| reqwest::blocking::multipart::Part::bytes(Vec::new())),
                );

            client.post(webhook_url).multipart(form).send()
        } else {
            let mut payload = serde_json::json!({
                "embeds": [{
                    "title": title,
                    "description": body,
                    "color": 5_814_783
                }]
            });
            if let Some(ref mention) = mention_content
                && let Some(obj) = payload.as_object_mut()
            {
                obj.insert("content".into(), serde_json::json!(mention));
            }
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
