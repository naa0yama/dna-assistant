//! Toast notification manager with duplicate suppression.
//!
//! Converts detection events into Windows Toast notifications, enforcing
//! a per-trigger cooldown to prevent notification flooding.
//! All timing parameters come from [`MonitorConfig`](crate::monitor::MonitorConfig).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use dna_detector::event::DetectionEvent;
use tracing::{debug, instrument, warn};

use crate::monitor::MonitorConfig;

/// Notification trigger kind, used as key for cooldown tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TriggerKind {
    SkillGreyed,
    DialogVisible,
    RoundGone,
    AllyHpLow,
}

/// Configuration for a notification trigger.
struct TriggerConfig {
    /// How long the condition must persist before notifying.
    sustain_duration: Duration,
    /// Per-trigger cooldown between repeated notifications.
    cooldown: Duration,
    /// Notification title.
    title: &'static str,
    /// Notification body.
    body: &'static str,
}

/// Build trigger config from `MonitorConfig` values.
const fn trigger_config(kind: TriggerKind, cfg: &MonitorConfig) -> TriggerConfig {
    match kind {
        TriggerKind::SkillGreyed => TriggerConfig {
            sustain_duration: cfg.notify_skill_sustain,
            cooldown: cfg.notification_cooldown,
            title: "Q スキル SP 枯渇",
            body: "味方がダウンした可能性があります",
        },
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
        TriggerKind::AllyHpLow => TriggerConfig {
            sustain_duration: cfg.notify_ally_hp_sustain,
            cooldown: cfg.notification_cooldown,
            title: "味方 HP 低下",
            body: "味方の HP が低下しています",
        },
    }
}

/// Manages notification triggers with sustain-time and cooldown logic.
///
/// Negative-state notifications (`RoundGone`, `SkillGreyed`) require that
/// the positive state (`RoundVisible`, `SkillReady`) was seen at least once
/// first. This prevents false notifications in the lobby or other screens
/// where the monitored UI elements are simply absent.
#[derive(Debug)]
pub struct NotificationManager {
    /// When each trigger condition first became active.
    condition_start: HashMap<TriggerKind, Instant>,
    /// When each trigger was last notified (for cooldown).
    last_notified: HashMap<TriggerKind, Instant>,
    /// Last time `RoundGone` was detected (for skill false-positive suppression).
    last_round_gone: Option<Instant>,
    /// True after `RoundVisible` is first seen. `RoundGone` notifications
    /// are suppressed until this becomes true.
    round_was_visible: bool,
    /// True when `RoundGone` has been notified and awaits `RoundVisible` reset.
    round_notified: bool,
    /// True after `SkillReady` is first seen. `SkillGreyed` notifications
    /// are suppressed until this becomes true.
    skill_was_ready: bool,
    /// Timing configuration.
    config: MonitorConfig,
}

impl NotificationManager {
    /// Create a new notification manager with the given configuration.
    pub fn new(config: &MonitorConfig) -> Self {
        Self {
            condition_start: HashMap::new(),
            last_notified: HashMap::new(),
            last_round_gone: None,
            round_was_visible: false,
            round_notified: false,
            skill_was_ready: false,
            config: config.clone(),
        }
    }

    /// Process detection events and send notifications if trigger conditions are met.
    #[instrument(skip_all)]
    pub fn process_events(&mut self, events: &[DetectionEvent]) {
        let now = Instant::now();

        for event in events {
            match event {
                DetectionEvent::SkillReady { .. } => {
                    self.skill_was_ready = true;
                    self.clear_condition(TriggerKind::SkillGreyed);
                }
                DetectionEvent::SkillGreyed { .. } => {
                    // Only track if skill was previously seen as Ready
                    // (prevents false notifications in lobby where icon is absent)
                    if self.skill_was_ready && !self.is_in_round_transition(now) {
                        self.track_condition(TriggerKind::SkillGreyed, now);
                    }
                }
                DetectionEvent::RoundVisible { .. } => {
                    self.round_was_visible = true;
                    self.round_notified = false;
                    self.clear_condition(TriggerKind::RoundGone);
                }
                DetectionEvent::RoundGone { .. } => {
                    self.last_round_gone = Some(now);
                    self.clear_condition(TriggerKind::SkillGreyed);
                    // Only track if round was previously visible and not already notified
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
                DetectionEvent::AllyHpLow { .. } => {
                    self.track_condition(TriggerKind::AllyHpLow, now);
                }
                DetectionEvent::AllyHpNormal { .. } => {
                    self.clear_condition(TriggerKind::AllyHpLow);
                }
            }
        }

        // Check all active conditions for sustained triggers
        let active_kinds: Vec<TriggerKind> = self.condition_start.keys().copied().collect();
        for kind in active_kinds {
            self.check_and_notify(kind, now);
        }
    }

    /// Check if we are within the round-transition suppression window.
    fn is_in_round_transition(&self, now: Instant) -> bool {
        self.last_round_gone
            .is_some_and(|t| now.duration_since(t) < self.config.round_transition_suppress)
    }

    /// Start tracking a condition (or keep existing start time).
    fn track_condition(&mut self, kind: TriggerKind, now: Instant) {
        self.condition_start.entry(kind).or_insert(now);
    }

    /// Clear a condition when the opposite event is received.
    fn clear_condition(&mut self, kind: TriggerKind) {
        self.condition_start.remove(&kind);
    }

    /// Check if a condition has been sustained long enough and send notification.
    fn check_and_notify(&mut self, kind: TriggerKind, now: Instant) {
        let Some(&start) = self.condition_start.get(&kind) else {
            return;
        };

        let tc = trigger_config(kind, &self.config);

        // Check sustain duration
        if now.duration_since(start) < tc.sustain_duration {
            return;
        }

        // Check per-trigger cooldown
        if let Some(&last) = self.last_notified.get(&kind)
            && now.duration_since(last) < tc.cooldown
        {
            return;
        }

        // Send notification
        Self::send_toast(tc.title, tc.body);
        self.last_notified.insert(kind, now);
        self.condition_start.remove(&kind);

        // RoundGone: one-shot until next RoundVisible
        if kind == TriggerKind::RoundGone {
            self.round_notified = true;
            self.round_was_visible = false;
        }
        // SkillGreyed: require SkillReady again before next notification
        if kind == TriggerKind::SkillGreyed {
            self.skill_was_ready = false;
        }
    }

    /// Send a Windows Toast notification via `notify-rust`.
    ///
    /// Uses the Tauri app identifier as `app_id` so Windows attributes the
    /// toast to DNA Assistant (not `PowerShell`). Sets `Default` sound and
    /// a 25-second timeout so the toast stays visible.
    fn send_toast(title: &str, body: &str) {
        debug!(title, body, "sending toast notification");

        let app_id = "com.naa0yama.dna-assistant";
        let result = notify_rust::Notification::new()
            .app_id(app_id)
            .summary(title)
            .body(body)
            .sound_name("Default")
            .timeout(notify_rust::Timeout::Milliseconds(25_000))
            .show();

        if let Err(e) = result {
            warn!(%e, "failed to send toast notification");
        }
    }
}
