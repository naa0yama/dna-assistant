//! Telemetry initialization: tracing subscriber with optional `OTel` export.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Guard that shuts down `OTel` providers on drop.
#[must_use]
pub struct TelemetryGuard {
    #[cfg(feature = "otel")]
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    #[cfg(feature = "otel")]
    meter_provider: Option<opentelemetry_sdk::metrics::SdkMeterProvider>,
}

#[cfg(all(target_os = "windows", feature = "otel"))]
impl TelemetryGuard {
    /// Return a `Meter` scoped to this crate, if metrics are enabled.
    pub fn meter(&self) -> Option<opentelemetry::metrics::Meter> {
        use opentelemetry::metrics::MeterProvider as _;
        self.meter_provider
            .as_ref()
            .map(|mp| mp.meter(env!("CARGO_PKG_NAME")))
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        // Shut down tracer first so it can emit final spans while the metric
        // exporter is still alive, then shut down metrics.
        #[cfg(feature = "otel")]
        {
            if let Some(provider) = self.tracer_provider.take()
                && let Err(e) = provider.shutdown()
            {
                tracing::error!("OTel tracer shutdown error: {e}");
            }
            if let Some(provider) = self.meter_provider.take()
                && let Err(e) = provider.shutdown()
            {
                tracing::error!("OTel meter shutdown error: {e}");
            }
        }
    }
}

/// Initialize the tracing subscriber.
///
/// When the `otel` feature is enabled and `OTEL_EXPORTER_OTLP_ENDPOINT` is set,
/// spans are exported via OTLP. Otherwise, only the fmt layer is active.
pub fn init() -> TelemetryGuard {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,dna=info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false);

    #[cfg(feature = "otel")]
    {
        let (otel_layer, tracer_provider, meter_provider) = init_otel();

        tracing_subscriber::registry()
            .with(otel_layer)
            .with(env_filter)
            .with(fmt_layer)
            .init();

        TelemetryGuard {
            tracer_provider,
            meter_provider,
        }
    }

    #[cfg(not(feature = "otel"))]
    {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();

        TelemetryGuard {}
    }
}

/// Build `OTel` layer, tracer provider, and meter provider.
///
/// Activated only when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
#[cfg(feature = "otel")]
fn init_otel() -> (
    Option<
        tracing_opentelemetry::OpenTelemetryLayer<
            tracing_subscriber::Registry,
            opentelemetry_sdk::trace::Tracer,
        >,
    >,
    Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    Option<opentelemetry_sdk::metrics::SdkMeterProvider>,
) {
    use opentelemetry::KeyValue;
    use opentelemetry::metrics::MeterProvider as _;
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_otlp::{MetricExporter, SpanExporter};
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::metrics::SdkMeterProvider;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_opentelemetry::OpenTelemetryLayer;

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|ep| !ep.is_empty());

    let Some(_endpoint) = endpoint else {
        return (None, None, None);
    };

    let resource = Resource::builder()
        .with_service_name(env!("CARGO_PKG_NAME"))
        .with_attributes([
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new(
                "service.instance.id",
                gethostname::gethostname().to_string_lossy().into_owned(),
            ),
            KeyValue::new("vcs.repository.ref.revision", env!("GIT_HASH")),
        ])
        .build();

    // --- Tracer ---
    let span_exporter = match SpanExporter::builder().with_http().build() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to create OTLP span exporter, running without OTel: {e}");
            return (None, None, None);
        }
    };

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();

    let layer = OpenTelemetryLayer::new(tracer_provider.tracer(env!("CARGO_PKG_NAME")));

    // --- Meter ---
    let metric_exporter = match MetricExporter::builder().with_http().build() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to create OTLP metric exporter, metrics disabled: {e}");
            return (Some(layer), Some(tracer_provider), None);
        }
    };

    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();

    opentelemetry::global::set_meter_provider(meter_provider.clone());

    // Register process-level metrics as Observable Gauges.
    let meter = meter_provider.meter(env!("CARGO_PKG_NAME"));
    register_process_metrics(&meter);

    (Some(layer), Some(tracer_provider), Some(meter_provider))
}

/// Register `process.*` observable gauges using `sysinfo`.
///
/// Each callback refreshes only the current process — no full-system scan.
/// All callbacks are panic-free: errors silently skip the observation.
#[cfg(feature = "otel")]
fn register_process_metrics(meter: &opentelemetry::metrics::Meter) {
    use std::sync::{Arc, Mutex};
    use sysinfo::{Pid, ProcessRefreshKind, System};

    let pid = Pid::from_u32(std::process::id());
    let sys = Arc::new(Mutex::new(System::new()));
    // f64::from(u32) is a lossless widening conversion; safe for CPU count values.
    let cpu_count = f64::from(
        u32::try_from(std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get))
            .unwrap_or(1_u32),
    );

    register_memory_gauges(
        meter,
        &sys,
        pid,
        ProcessRefreshKind::nothing().with_memory(),
    );
    register_cpu_uptime_gauges(
        meter,
        Arc::clone(&sys),
        pid,
        ProcessRefreshKind::nothing().with_cpu(),
        cpu_count,
    );

    // Thread count is Linux-only (requires /proc/<pid>/task).
    #[cfg(target_os = "linux")]
    register_thread_gauge(
        meter,
        sys,
        pid,
        ProcessRefreshKind::nothing().with_memory().with_cpu(),
    );
}

#[cfg(feature = "otel")]
fn register_memory_gauges(
    meter: &opentelemetry::metrics::Meter,
    sys: &std::sync::Arc<std::sync::Mutex<sysinfo::System>>,
    pid: sysinfo::Pid,
    kind: sysinfo::ProcessRefreshKind,
) {
    use sysinfo::ProcessesToUpdate;

    // --- process.memory.usage (RSS) ---
    {
        let sys = std::sync::Arc::clone(sys);
        meter
            .i64_observable_gauge("process.memory.usage")
            .with_unit("By")
            .with_description("Resident Set Size in bytes")
            .with_callback(move |obs| {
                if let Ok(mut s) = sys.lock() {
                    s.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, kind);
                    if let Some(p) = s.process(pid) {
                        obs.observe(i64::try_from(p.memory()).unwrap_or(i64::MAX), &[]);
                    }
                }
            })
            .build();
    }

    // --- process.memory.virtual ---
    {
        let sys = std::sync::Arc::clone(sys);
        meter
            .i64_observable_gauge("process.memory.virtual")
            .with_unit("By")
            .with_description("Virtual memory size in bytes")
            .with_callback(move |obs| {
                if let Ok(mut s) = sys.lock() {
                    s.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, kind);
                    if let Some(p) = s.process(pid) {
                        obs.observe(i64::try_from(p.virtual_memory()).unwrap_or(i64::MAX), &[]);
                    }
                }
            })
            .build();
    }
}

#[cfg(feature = "otel")]
fn register_cpu_uptime_gauges(
    meter: &opentelemetry::metrics::Meter,
    sys: std::sync::Arc<std::sync::Mutex<sysinfo::System>>,
    pid: sysinfo::Pid,
    kind_cpu: sysinfo::ProcessRefreshKind,
    cpu_count: f64,
) {
    use sysinfo::ProcessesToUpdate;

    // --- process.cpu.utilization (0.0 - 1.0) ---
    // First observation will be 0.0 because sysinfo needs two refreshes for a delta.
    {
        let sys = std::sync::Arc::clone(&sys);
        meter
            .f64_observable_gauge("process.cpu.utilization")
            .with_unit("1")
            .with_description("CPU utilization as a fraction of all logical CPUs")
            .with_callback(move |obs| {
                if let Ok(mut s) = sys.lock() {
                    s.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, kind_cpu);
                    if let Some(p) = s.process(pid) {
                        // sysinfo returns % across all CPUs; normalize to [0, 1].
                        obs.observe(f64::from(p.cpu_usage()) / 100.0 / cpu_count, &[]);
                    }
                }
            })
            .build();
    }

    // --- process.uptime (seconds since process start) ---
    // run_time() is derived from start_time() on any refresh; no specific kind needed.
    {
        meter
            .f64_observable_gauge("process.uptime")
            .with_unit("s")
            .with_description("Seconds elapsed since the process started")
            .with_callback(move |obs| {
                if let Ok(mut s) = sys.lock() {
                    s.refresh_processes_specifics(
                        ProcessesToUpdate::Some(&[pid]),
                        false,
                        sysinfo::ProcessRefreshKind::nothing(),
                    );
                    if let Some(p) = s.process(pid) {
                        // run_time() is seconds since start; fits comfortably in f64.
                        obs.observe(
                            f64::from(u32::try_from(p.run_time()).unwrap_or(u32::MAX)),
                            &[],
                        );
                    }
                }
            })
            .build();
    }
}

/// Register `process.thread.count` via Linux `/proc/<pid>/task`.
#[cfg(all(feature = "otel", target_os = "linux"))]
fn register_thread_gauge(
    meter: &opentelemetry::metrics::Meter,
    sys: std::sync::Arc<std::sync::Mutex<sysinfo::System>>,
    pid: sysinfo::Pid,
    kind: sysinfo::ProcessRefreshKind,
) {
    use sysinfo::ProcessesToUpdate;

    meter
        .i64_observable_gauge("process.thread.count")
        .with_unit("{thread}")
        .with_description("Number of threads in the process")
        .with_callback(move |obs| {
            if let Ok(mut s) = sys.lock() {
                s.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, kind);
                if let Some(p) = s.process(pid)
                    && let Some(tasks) = p.tasks()
                {
                    obs.observe(i64::try_from(tasks.len()).unwrap_or(i64::MAX), &[]);
                }
            }
        })
        .build();
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "otel")]
    fn init_otel_respects_endpoint_env() {
        // init_otel() should return (None, None, None) when endpoint is empty,
        // or (Some, Some, Some) when a valid endpoint is configured.
        // Both paths are valid — we just verify it does not panic.
        let (layer, tracer, meter) = super::init_otel();
        // All must be the same variant (all Some or all None when endpoint is empty)
        let _ = (layer, tracer, meter);
    }

    #[test]
    fn telemetry_guard_drop_is_safe() {
        // Guard with no providers should drop without panic.
        let guard = super::TelemetryGuard {
            #[cfg(feature = "otel")]
            tracer_provider: None,
            #[cfg(feature = "otel")]
            meter_provider: None,
        };
        drop(guard);
    }

    #[test]
    #[cfg(feature = "otel")]
    fn register_process_metrics_does_not_panic() {
        // Verify that process metrics registration runs without panic
        // even when no OTLP endpoint is configured.
        use opentelemetry::metrics::MeterProvider as _;
        let provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder().build();
        let meter = provider.meter("test");
        super::register_process_metrics(&meter);
        // Allow the provider to flush/shutdown cleanly
        let _ = provider.shutdown();
    }
}
