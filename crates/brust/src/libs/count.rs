//! Iteration runner for metrics demonstration.

use std::thread;
use std::time::Duration;

/// Result of a single iteration.
#[derive(Debug)]
pub struct IterationResult {
    /// Duration of the random delay in seconds.
    pub duration_secs: u64,
}

/// Run `count` iterations, each sleeping for a random 1-5 seconds.
///
/// Logs progress via `tracing::info!` on each iteration.
/// Returns a `Vec<IterationResult>` recording each iteration's delay.
pub fn run_iterations(count: u32) -> Vec<IterationResult> {
    #[allow(clippy::as_conversions)] // u32 -> usize is always safe (usize >= 32 bits)
    let mut results = Vec::with_capacity(count as usize);

    for i in 1..=count {
        let secs = rand::random_range(1..=5);
        tracing::info!(iteration = i, delay_secs = secs, "starting iteration");
        thread::sleep(Duration::from_secs(secs));
        tracing::info!(iteration = i, delay_secs = secs, "finished iteration");
        results.push(IterationResult {
            duration_secs: secs,
        });
    }

    results
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn test_run_iterations_returns_correct_count() {
        let results = run_iterations(2);
        assert_eq!(results.len(), 2);
        assert!((1..=5).contains(&results[0].duration_secs));
        assert!((1..=5).contains(&results[1].duration_secs));
    }

    #[test]
    fn test_run_iterations_duration_range() {
        let results = run_iterations(1);
        assert_eq!(results.len(), 1);
        assert!(
            (1..=5).contains(&results[0].duration_secs),
            "duration_secs should be between 1 and 5, got {}",
            results[0].duration_secs
        );
    }

    #[test]
    fn test_run_iterations_zero() {
        let results = run_iterations(0);
        assert!(results.is_empty());
    }
}
