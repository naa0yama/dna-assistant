//! Application-specific metrics for dna-assistant.
//!
//! All instruments are created once via `install` and accessed globally via `get`.
//! When `OTEL_EXPORTER_OTLP_ENDPOINT` is unset, `install` is never called and
//! `get` returns `None`, so every call site has zero overhead on the hot path.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use opentelemetry::metrics::{Counter, Histogram, Meter};

#[cfg(feature = "otel")]
use crate::telemetry::conventions::metric as dna_metric;

static APP_METRICS: OnceLock<AppMetrics> = OnceLock::new();

/// Install the global `AppMetrics` instance from `meter`.
///
/// Must be called at most once — subsequent calls are no-ops (enforced by `OnceLock`).
pub fn install(meter: &Meter) {
    let _ = APP_METRICS.get_or_init(|| AppMetrics::new(meter));
}

/// Return the global `AppMetrics`, or `None` when `OTel` is disabled.
#[must_use]
pub fn get() -> Option<&'static AppMetrics> {
    APP_METRICS.get()
}

/// Application-level instrumentation instruments.
///
/// Observable gauges for state-derived values (`result_scanning_active`,
/// `select_votes_len`) are backed by `Arc<Atomic*>` fields. Callers update
/// the atomics; the `OTel` SDK reads them on each export interval.
pub struct AppMetrics {
    // --- Monitor loop ---
    pub monitor_loop_iterations: Counter<u64>,
    pub monitor_loop_duration: Histogram<f64>,

    // --- Capture ---
    pub capture_frames: Counter<u64>,
    pub capture_duration: Histogram<f64>,

    // --- Detection ---
    pub detection_events: Counter<u64>,

    // --- #1 result_scanning (ObservableGauge backed by atomic) ---
    /// Set to `true` while result-scanning OCR is active; `false` otherwise.
    pub result_scanning: Arc<AtomicBool>,

    // --- #1 / #2 OCR ---
    pub ocr_calls: Counter<u64>,
    pub ocr_duration: Histogram<f64>,

    // --- #2 select_round_votes ---
    pub select_votes_len: Arc<AtomicUsize>,
    pub select_votes_pushes: Counter<u64>,
    pub select_votes_clears: Counter<u64>,

    // --- WGC ---
    pub wgc_frames_received: Counter<u64>,
    pub wgc_capturer_started: Counter<u64>,
    pub wgc_capturer_dropped: Counter<u64>,
}

impl AppMetrics {
    fn new(meter: &Meter) -> Self {
        // Atomics shared with ObservableGauge callbacks.
        let result_scanning = Arc::new(AtomicBool::new(false));
        let select_votes_len = Arc::new(AtomicUsize::new(0));

        // --- Observable gauge: result_scanning_active ---
        {
            let flag = Arc::clone(&result_scanning);
            meter
                .i64_observable_gauge(dna_metric::MONITOR_RESULT_SCANNING_ACTIVE)
                .with_unit("1")
                .with_description("1 when result-screen OCR scanning is active, 0 otherwise")
                .with_callback(move |obs| {
                    obs.observe(i64::from(flag.load(Ordering::Relaxed)), &[]);
                })
                .build();
        }

        // --- Observable gauge: select_round_votes.len ---
        {
            let len = Arc::clone(&select_votes_len);
            meter
                .i64_observable_gauge(dna_metric::MONITOR_SELECT_ROUND_VOTES_LEN)
                .with_unit("{vote}")
                .with_description("Current length of the select_round_votes buffer")
                .with_callback(move |obs| {
                    obs.observe(
                        i64::try_from(len.load(Ordering::Relaxed)).unwrap_or(i64::MAX),
                        &[],
                    );
                })
                .build();
        }

        Self {
            // Monitor loop
            monitor_loop_iterations: meter
                .u64_counter(dna_metric::MONITOR_LOOP_ITERATIONS)
                .with_unit("{iteration}")
                .with_description("Total monitor loop iterations")
                .build(),
            monitor_loop_duration: meter
                .f64_histogram(dna_metric::MONITOR_LOOP_DURATION)
                .with_unit("s")
                .with_description("Duration of each monitor loop iteration")
                .build(),

            // Capture
            capture_frames: meter
                .u64_counter(dna_metric::CAPTURE_FRAMES)
                .with_unit("{frame}")
                .with_description("Total captured frames")
                .build(),
            capture_duration: meter
                .f64_histogram(dna_metric::CAPTURE_DURATION)
                .with_unit("s")
                .with_description("Duration of each screen capture")
                .build(),

            // Detection
            detection_events: meter
                .u64_counter(dna_metric::DETECTION_EVENTS)
                .with_unit("{event}")
                .with_description("Total detection events fired, by detector")
                .build(),

            // result_scanning state
            result_scanning,

            // OCR
            ocr_calls: meter
                .u64_counter(dna_metric::OCR_CALLS)
                .with_unit("{call}")
                .with_description("Total OCR recognize calls, by kind and ROI")
                .build(),
            ocr_duration: meter
                .f64_histogram(dna_metric::OCR_DURATION)
                .with_unit("s")
                .with_description("Duration of OCR recognize calls")
                .build(),

            // select_round_votes
            select_votes_len,
            select_votes_pushes: meter
                .u64_counter(dna_metric::MONITOR_SELECT_ROUND_VOTES_PUSHES)
                .with_unit("{vote}")
                .with_description("Total pushes to select_round_votes")
                .build(),
            select_votes_clears: meter
                .u64_counter(dna_metric::MONITOR_SELECT_ROUND_VOTES_CLEARS)
                .with_unit("{clear}")
                .with_description("Total clears of select_round_votes")
                .build(),

            // WGC
            wgc_frames_received: meter
                .u64_counter(dna_metric::WGC_FRAMES_RECEIVED)
                .with_unit("{frame}")
                .with_description("Total frames received from WGC callback")
                .build(),
            wgc_capturer_started: meter
                .u64_counter(dna_metric::WGC_CAPTURER_STARTED)
                .with_unit("{capturer}")
                .with_description("Total WGC Capturer instances started")
                .build(),
            wgc_capturer_dropped: meter
                .u64_counter(dna_metric::WGC_CAPTURER_DROPPED)
                .with_unit("{capturer}")
                .with_description(
                    "Total WGC Capturer instances dropped (started - dropped = leaked)",
                )
                .build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::metrics::MeterProvider as _;
    use opentelemetry_sdk::metrics::SdkMeterProvider;

    #[test]
    fn install_is_idempotent() {
        // Two installs must not panic; second is silently ignored.
        let provider = SdkMeterProvider::builder().build();
        let meter = provider.meter("test");
        install(&meter);
        install(&meter); // second call must be a no-op
        let _ = provider.shutdown();
    }

    #[test]
    fn get_returns_some_after_install() {
        // get() should return Some once install() has been called.
        // Note: APP_METRICS is a process-wide singleton so this test relies on
        // install() having been called in the same test binary (possibly by
        // install_is_idempotent above). Run with --test-threads=1 if ordering matters.
        assert!(get().is_some());
    }

    #[test]
    fn atomic_flags_observable() {
        if let Some(m) = get() {
            m.result_scanning.store(true, Ordering::Relaxed);
            assert!(m.result_scanning.load(Ordering::Relaxed));
            m.result_scanning.store(false, Ordering::Relaxed);
        }
    }
}
