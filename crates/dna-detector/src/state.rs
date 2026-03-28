//! Debounce wrapper for detectors to suppress transient state changes.

use std::time::{Duration, Instant};

use image::RgbaImage;
use tracing::instrument;

use crate::detector::Detector;
use crate::event::DetectionEvent;

/// Wraps a detector to suppress events during a cooldown period.
///
/// After an event is emitted, subsequent events of the same variant
/// are suppressed until the cooldown expires.
#[derive(Debug)]
pub struct DebouncedDetector<D> {
    inner: D,
    cooldown: Duration,
    last_event_time: Option<Instant>,
}

impl<D: Detector> DebouncedDetector<D> {
    /// Create a debounced wrapper with the specified cooldown duration.
    #[must_use]
    pub const fn new(inner: D, cooldown: Duration) -> Self {
        Self {
            inner,
            cooldown,
            last_event_time: None,
        }
    }

    /// Analyze a frame, returning events only if the cooldown has expired.
    #[instrument(skip_all, name = "debounced_process")]
    pub fn process(&mut self, frame: &RgbaImage) -> Vec<DetectionEvent> {
        let events = self.inner.analyze(frame);
        if events.is_empty() {
            return events;
        }

        let now = Instant::now();
        if let Some(last) = self.last_event_time
            && now.duration_since(last) < self.cooldown
        {
            return Vec::new();
        }

        self.last_event_time = Some(now);
        events
    }

    /// Reset the cooldown timer, allowing the next event through immediately.
    pub const fn reset(&mut self) {
        self.last_event_time = None;
    }
}
