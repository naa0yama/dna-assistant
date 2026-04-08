//! OpenTelemetry instrumentation root module.
//!
//! Hosts cross-signal conventions and signal-specific submodules.
//! Add `tracing` / `logs` submodules here when adopting those signals.
//!
//! Telemetry initialization: tracing subscriber with optional `OTel` export.

#[cfg(all(target_os = "windows", feature = "otel"))]
pub mod conventions;
#[cfg(all(target_os = "windows", feature = "otel"))]
pub mod metrics;

use tracing::Subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;

/// Type-erased handle for reloading the log-level filter at runtime.
///
/// Wraps a `reload::Handle<EnvFilter, _>` with type erasure so it can be
/// stored as Tauri managed state regardless of the concrete subscriber type,
/// which varies with the `otel` feature flag.
///
/// Obtained from [`init`] and stored via `app.manage()` so that
/// [`crate::commands::save_settings`] can hot-reload `RUST_LOG` on save.
// Fields and methods are used only on Windows (commands.rs is cfg(windows)).
#[allow(dead_code)]
pub struct EnvFilterHandle(std::sync::Arc<dyn Fn(EnvFilter) -> Result<(), String> + Send + Sync>);

impl EnvFilterHandle {
    fn from_handle<S>(handle: reload::Handle<EnvFilter, S>) -> Self
    where
        S: Subscriber + 'static,
    {
        Self(std::sync::Arc::new(move |filter| {
            handle.reload(filter).map_err(|e| e.to_string())
        }))
    }

    /// Replace the active `EnvFilter`. Returns an error description on failure.
    #[allow(dead_code)] // Used only on Windows (commands.rs is cfg(windows))
    pub fn reload(&self, filter: EnvFilter) -> Result<(), String> {
        (self.0)(filter)
    }
}

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

/// Runtime overrides supplied by the caller before the telemetry layer starts.
///
/// Each field is optional via empty string: an empty `rust_log` falls back to
/// the `RUST_LOG` environment variable (then `"warn,dna=info"`); empty
/// `otel_endpoint` and `otel_headers` fall back to the standard
/// `OTEL_EXPORTER_OTLP_*` environment variables handled by the SDK.
#[derive(Default)]
pub struct TelemetryOverrides<'a> {
    /// `RUST_LOG`-style directive string, e.g. `"debug,dna=trace"`.
    pub rust_log: &'a str,
    /// OTLP HTTP endpoint, e.g. `"http://localhost:4318"`.
    pub otel_endpoint: &'a str,
    /// Comma-separated headers as `key=value,key=value`.
    pub otel_headers: &'a str,
}

/// Initialize the tracing subscriber.
///
/// When the `otel` feature is enabled and an OTLP endpoint is configured
/// (via `overrides.otel_endpoint` or the `OTEL_EXPORTER_OTLP_ENDPOINT`
/// environment variable), spans are exported via OTLP. Otherwise, only the
/// fmt layer is active.
///
/// Returns `(TelemetryGuard, EnvFilterHandle)`.  The handle lets callers
/// reload the `EnvFilter` at runtime (e.g. when the user saves `debug_rust_log`
/// in Settings) without restarting the process.
pub fn init(overrides: &TelemetryOverrides<'_>) -> (TelemetryGuard, EnvFilterHandle) {
    let env_filter = if overrides.rust_log.is_empty() {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,dna=info"))
    } else {
        EnvFilter::new(overrides.rust_log)
    };
    let (filter_layer, handle) = reload::Layer::new(env_filter);

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false);

    #[cfg(feature = "otel")]
    {
        let (otel_layer, tracer_provider, meter_provider) =
            init_otel(overrides.otel_endpoint, overrides.otel_headers);

        // otel_layer is placed first (.with innermost) so its S = Registry,
        // matching OpenTelemetryLayer<Registry, Tracer>: Layer<Registry>.
        // filter_layer then wraps the otel-layered registry.
        tracing_subscriber::registry()
            .with(otel_layer)
            .with(filter_layer)
            .with(fmt_layer)
            .init();

        (
            TelemetryGuard {
                tracer_provider,
                meter_provider,
            },
            EnvFilterHandle::from_handle(handle),
        )
    }

    #[cfg(not(feature = "otel"))]
    {
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(fmt_layer)
            .init();

        (TelemetryGuard {}, EnvFilterHandle::from_handle(handle))
    }
}

/// Normalise a user-supplied OTLP endpoint string into a base URL.
///
/// The OTLP HTTP exporter's `.with_endpoint()` treats its argument as the
/// **full signal URL** (no per-signal path is appended), whereas the
/// `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable is treated as a base URL
/// and has `/v1/traces` / `/v1/metrics` appended automatically by the SDK.
///
/// This function strips any known signal-specific suffix so that callers can
/// construct the correct per-signal URL unconditionally:
///
/// - `http://host:5080/api/default`         → unchanged (already a base URL)
/// - `http://host:5080/api/default/`        → `http://host:5080/api/default`
/// - `http://host:5080/api/default/v1/traces` → `http://host:5080/api/default`
/// - `http://host:5080/api/default/v1/metrics` → `http://host:5080/api/default`
#[cfg(feature = "otel")]
fn normalize_otlp_base(endpoint: &str) -> String {
    const SIGNAL_SUFFIXES: &[&str] = &["/v1/traces", "/v1/metrics", "/v1/logs"];

    let trimmed = endpoint.trim_end_matches('/');
    for suffix in SIGNAL_SUFFIXES {
        if let Some(base) = trimmed.strip_suffix(suffix) {
            return base.trim_end_matches('/').to_owned();
        }
    }
    trimmed.to_owned()
}

/// Build `OTel` layer, tracer provider, and meter provider.
///
/// `endpoint_override` is treated as a base URL: any trailing `/v1/traces`,
/// `/v1/metrics`, or `/v1/logs` suffix is stripped automatically, so both
/// `"http://host:4318"` and `"http://host:4318/v1/traces"` are accepted.
/// An empty string falls back to the `OTEL_EXPORTER_OTLP_ENDPOINT` environment
/// variable (where the SDK appends the signal path as per the `OTel` spec).
/// `headers_override` is parsed as comma-separated `key=value` pairs and takes
/// precedence over the `OTEL_EXPORTER_OTLP_HEADERS` environment variable; an
/// empty string lets the SDK read the env var.
/// Returns `(None, None, None)` when no endpoint is configured.
#[cfg(feature = "otel")]
fn init_otel(
    endpoint_override: &str,
    headers_override: &str,
) -> (
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
    use opentelemetry_otlp::{MetricExporter, SpanExporter, WithExportConfig, WithHttpConfig};
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::metrics::SdkMeterProvider;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_opentelemetry::OpenTelemetryLayer;

    // Resolve endpoint: explicit override > env var > disabled.
    let endpoint = if endpoint_override.is_empty() {
        std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|ep| !ep.is_empty())
    } else {
        Some(endpoint_override.to_owned())
    };

    let Some(endpoint) = endpoint else {
        return (None, None, None);
    };

    let base = normalize_otlp_base(&endpoint);
    let traces_endpoint = format!("{base}/v1/traces");
    let metrics_endpoint = format!("{base}/v1/metrics");

    let header_map = parse_otlp_headers(headers_override);

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
    let mut span_builder = SpanExporter::builder()
        .with_http()
        .with_endpoint(traces_endpoint);
    if !header_map.is_empty() {
        span_builder = span_builder.with_headers(header_map.clone());
    }
    let span_exporter = match span_builder.build() {
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
    let mut metric_builder = MetricExporter::builder()
        .with_http()
        .with_endpoint(metrics_endpoint);
    if !header_map.is_empty() {
        metric_builder = metric_builder.with_headers(header_map);
    }
    let metric_exporter = match metric_builder.build() {
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

/// Parse comma-separated `key=value` OTLP header pairs into a `HashMap`.
///
/// Whitespace around keys and values is trimmed. Entries that do not contain
/// `=` or have an empty key are skipped. An empty `raw` string returns an
/// empty map, which tells the builder to leave headers unset so the SDK falls
/// back to the `OTEL_EXPORTER_OTLP_HEADERS` environment variable.
#[cfg(feature = "otel")]
fn parse_otlp_headers(raw: &str) -> std::collections::HashMap<String, String> {
    if raw.is_empty() {
        return std::collections::HashMap::new();
    }
    raw.split(',')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?.trim();
            let value = parts.next()?.trim();
            if key.is_empty() {
                return None;
            }
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
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
    fn normalize_otlp_base_unchanged() {
        assert_eq!(
            super::normalize_otlp_base("http://h:5080/api/default"),
            "http://h:5080/api/default"
        );
    }

    #[test]
    #[cfg(feature = "otel")]
    fn normalize_otlp_base_strips_trailing_slash() {
        assert_eq!(
            super::normalize_otlp_base("http://h:5080/api/default/"),
            "http://h:5080/api/default"
        );
    }

    #[test]
    #[cfg(feature = "otel")]
    fn normalize_otlp_base_strips_v1_traces() {
        assert_eq!(
            super::normalize_otlp_base("http://h:5080/api/default/v1/traces"),
            "http://h:5080/api/default"
        );
    }

    #[test]
    #[cfg(feature = "otel")]
    fn normalize_otlp_base_strips_v1_metrics() {
        assert_eq!(
            super::normalize_otlp_base("http://h:5080/api/default/v1/metrics"),
            "http://h:5080/api/default"
        );
    }

    #[test]
    #[cfg(feature = "otel")]
    fn normalize_otlp_base_strips_v1_logs() {
        assert_eq!(
            super::normalize_otlp_base("http://h:5080/api/default/v1/logs"),
            "http://h:5080/api/default"
        );
    }

    #[test]
    #[cfg(feature = "otel")]
    fn normalize_otlp_base_strips_suffix_with_trailing_slash() {
        assert_eq!(
            super::normalize_otlp_base("http://h:5080/api/default/v1/traces/"),
            "http://h:5080/api/default"
        );
    }

    #[test]
    #[cfg(feature = "otel")]
    fn init_otel_returns_none_when_no_endpoint() {
        // This test requires OTEL_EXPORTER_OTLP_ENDPOINT to be unset.
        // Skip if it is already set in the environment (e.g. CI / dev env).
        if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
            return;
        }
        let (layer, tracer, meter) = super::init_otel("", "");
        assert!(layer.is_none());
        assert!(tracer.is_none());
        assert!(meter.is_none());
    }

    #[test]
    #[cfg(feature = "otel")]
    fn parse_otlp_headers_empty() {
        assert!(super::parse_otlp_headers("").is_empty());
    }

    #[test]
    #[cfg(feature = "otel")]
    fn parse_otlp_headers_single() {
        let map = super::parse_otlp_headers("x-api-key=secret");
        assert_eq!(map.get("x-api-key").map(String::as_str), Some("secret"));
        assert_eq!(map.len(), 1);
    }

    #[test]
    #[cfg(feature = "otel")]
    fn parse_otlp_headers_multiple_with_whitespace() {
        let map = super::parse_otlp_headers(" k1 = v1 , k2=v2 ");
        assert_eq!(map.get("k1").map(String::as_str), Some("v1"));
        assert_eq!(map.get("k2").map(String::as_str), Some("v2"));
    }

    #[test]
    #[cfg(feature = "otel")]
    fn parse_otlp_headers_skips_malformed() {
        // Entry without '=' should be dropped; valid entry still parsed.
        let map = super::parse_otlp_headers("no-equals,k=v");
        assert!(!map.contains_key("no-equals"));
        assert_eq!(map.get("k").map(String::as_str), Some("v"));
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
