//! dna-prefixed semantic conventions for app-specific telemetry.
//!
//! Mirrors the layout of `opentelemetry_semantic_conventions::{metric,
//! attribute}` to provide a single source of truth for `dna.*` names
//! across all signals (metrics today, tracing/logs in the future).
//! Use these constants instead of string literals to avoid typos and drift.

pub mod metric {
    // Monitor loop
    pub const MONITOR_LOOP_ITERATIONS: &str = "dna.monitor.loop.iterations";
    pub const MONITOR_LOOP_DURATION: &str = "dna.monitor.loop.duration";
    pub const MONITOR_RESULT_SCANNING_ACTIVE: &str = "dna.monitor.result_scanning.active";
    pub const MONITOR_SELECT_ROUND_VOTES_LEN: &str = "dna.monitor.select_round_votes.len";
    pub const MONITOR_SELECT_ROUND_VOTES_PUSHES: &str = "dna.monitor.select_round_votes.pushes";
    pub const MONITOR_SELECT_ROUND_VOTES_CLEARS: &str = "dna.monitor.select_round_votes.clears";

    // Capture
    pub const CAPTURE_FRAMES: &str = "dna.capture.frames";
    pub const CAPTURE_DURATION: &str = "dna.capture.duration";

    // Detection
    pub const DETECTION_EVENTS: &str = "dna.detection.events";

    // OCR
    pub const OCR_CALLS: &str = "dna.ocr.calls";
    pub const OCR_DURATION: &str = "dna.ocr.duration";

    // WGC
    pub const WGC_FRAMES_RECEIVED: &str = "dna.wgc.frames_received";
    pub const WGC_CAPTURER_STARTED: &str = "dna.wgc.capturer.started";
    pub const WGC_CAPTURER_DROPPED: &str = "dna.wgc.capturer.dropped";
}

pub mod attribute {
    /// OCR call kind: `round_number` or `result_screen`.
    pub const KIND: &str = "dna.kind";
}

/// Attribute values for [`attribute::KIND`].
pub mod kind {
    pub const ROUND_NUMBER: &str = "round_number";
    pub const RESULT_SCREEN: &str = "result_screen";
}
