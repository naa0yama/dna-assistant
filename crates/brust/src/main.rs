//! Brust - Rust ボイラープレートプロジェクト

/// ライブラリモジュール群
pub mod libs;
use clap::Parser;
use tracing_subscriber::filter::EnvFilter;
#[cfg(not(feature = "otel"))]
use tracing_subscriber::fmt;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "otel")]
use tracing_subscriber::util::SubscriberInitExt;

use crate::libs::count;
use crate::libs::hello::{GreetingError, sayhello};

#[derive(Parser)]
#[command(about, version = APP_VERSION)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long, default_value = "Youre")]
    name: String,
    /// Gender for greeting (man, woman)
    #[arg(short, long)]
    gender: Option<String>,
    /// Number of iterations to run with random delays (metrics demo)
    #[arg(short = 'c', long = "count")]
    count: Option<u32>,
}

const APP_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (rev:", env!("GIT_HASH"), ")",);

fn main() {
    #[cfg(not(feature = "otel"))]
    {
        fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .init();
    }

    #[cfg(feature = "otel")]
    let otel_providers = init_otel();

    let args = Args::parse();

    run(&args.name, args.gender.as_deref());

    if let Some(count) = args.count {
        run_count(count);
    }

    #[cfg(feature = "otel")]
    shutdown_otel(otel_providers);
}

/// Providers returned by `OTel` initialization for shutdown.
#[cfg(feature = "otel")]
type OtelProviders = (
    Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    Option<opentelemetry_sdk::metrics::SdkMeterProvider>,
    Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
);

// NOTEST(cfg): OTel init requires OTLP endpoint — covered by integration trace tests
/// Initialize `OTel` tracing, logging, and metrics providers.
#[cfg(feature = "otel")]
fn init_otel() -> OtelProviders {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,opentelemetry=off"));
    let fmt_layer = tracing_subscriber::fmt::layer();

    let (otel_trace_layer, tp, mp, lp, otel_log_layer) =
        std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|ep| !ep.is_empty())
            .and_then(|_| {
                let resource = opentelemetry_sdk::Resource::builder()
                    .with_service_name(env!("CARGO_PKG_NAME"))
                    .build();

                // --- Traces ---
                let span_exporter = opentelemetry_otlp::SpanExporter::builder()
                    .with_http()
                    .build()
                    .ok()?;
                let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                    .with_resource(resource.clone())
                    .with_simple_exporter(span_exporter)
                    .build();
                let tracer = opentelemetry::trace::TracerProvider::tracer(
                    &tracer_provider,
                    env!("CARGO_PKG_NAME"),
                );
                let trace_layer = tracing_opentelemetry::layer().with_tracer(tracer);

                // --- Logs ---
                let log_exporter = opentelemetry_otlp::LogExporter::builder()
                    .with_http()
                    .build()
                    .ok()?;
                let logger_provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                    .with_resource(resource.clone())
                    .with_simple_exporter(log_exporter)
                    .build();
                let log_layer =
                    opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(
                        &logger_provider,
                    );

                // --- Metrics (last: consumes resource without clone) ---
                let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
                    .with_http()
                    .build()
                    .ok()?;
                let metric_reader =
                    opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter)
                        .with_interval(std::time::Duration::from_secs(5))
                        .build();
                let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
                    .with_resource(resource)
                    .with_reader(metric_reader)
                    .build();
                opentelemetry::global::set_meter_provider(meter_provider.clone());

                Some((
                    Some(trace_layer),
                    Some(tracer_provider),
                    Some(meter_provider),
                    Some(logger_provider),
                    Some(log_layer),
                ))
            })
            .unwrap_or((None, None, None, None, None));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .init();

    (tp, mp, lp)
}

// NOTEST(cfg): OTel shutdown requires live providers — covered by integration trace tests
/// Shut down `OTel` providers in reverse initialization order.
#[cfg(feature = "otel")]
fn shutdown_otel((tracer_provider, meter_provider, logger_provider): OtelProviders) {
    if let Some(provider) = tracer_provider
        && let Err(e) = provider.shutdown()
    {
        tracing::warn!("failed to shutdown OTel tracer provider: {e}");
    }
    if let Some(provider) = meter_provider {
        if let Err(e) = provider.force_flush() {
            tracing::warn!("failed to flush OTel meter provider: {e}");
        }
        if let Err(e) = provider.shutdown() {
            tracing::warn!("failed to shutdown OTel meter provider: {e}");
        }
    }
    if let Some(provider) = logger_provider
        && let Err(e) = provider.shutdown()
    {
        tracing::warn!("failed to shutdown OTel logger provider: {e}");
    }
}

/// アプリケーションのメイン処理を実行
///
/// # Arguments
/// * `name` - 挨拶対象の名前
/// * `gender` - 性別オプション（None, Some("man"), Some("woman"), その他）
#[cfg_attr(feature = "otel", tracing::instrument)]
pub fn run(name: &str, gender: Option<&str>) {
    let result = sayhello(name, gender);
    let greeting = format_greeting(name, result);
    tracing::info!("{}, new world!!", greeting);
}

/// Format a greeting from a `sayhello` result, handling errors gracefully.
fn format_greeting(name: &str, result: Result<String, GreetingError>) -> String {
    match result {
        Ok(msg) => msg,
        Err(GreetingError::InvalidGender(invalid_gender)) => {
            tracing::warn!(
                "Invalid gender '{}' specified, using default greeting",
                invalid_gender
            );
            format!("Hi, {name} (invalid gender: {invalid_gender})")
        }
        Err(GreetingError::UnknownGender) => {
            tracing::error!("Unexpected error in greeting generation, using default");
            format!("Hi, {name}")
        }
    }
}

/// Run iteration count demo and record `OTel` metrics.
#[cfg_attr(feature = "otel", tracing::instrument)]
fn run_count(count: u32) {
    let results = count::run_iterations(count);

    #[cfg(feature = "otel")]
    {
        let meter = opentelemetry::global::meter(env!("CARGO_PKG_NAME"));
        let counter = meter.u64_counter("iteration.count").build();
        let histogram = meter
            .f64_histogram("iteration.duration")
            .with_unit("s")
            .build();

        for result in &results {
            counter.add(1, &[]);
            #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
            // u64 seconds (1..=5) fits f64 losslessly
            histogram.record(result.duration_secs as f64, &[]);
        }
    }

    #[cfg(not(feature = "otel"))]
    drop(results);
}

#[cfg(test)]
mod tests {
    use super::{format_greeting, run};
    use crate::libs::hello::GreetingError;
    use tracing::subscriber::with_default;
    use tracing_mock::{expect, subscriber};

    /// Build a mock subscriber that expects the `run` instrumentation span
    /// wrapping a single event with the given message.
    fn mock_run_single_event(msg: &str) -> (impl tracing::Subscriber, subscriber::MockHandle) {
        let run_span = expect::span().named("run");
        subscriber::mock()
            .new_span(run_span.clone())
            .enter(run_span.clone())
            .event(expect::event().with_fields(expect::msg(msg)))
            .exit(run_span.clone())
            .drop_span(run_span)
            .only()
            .run_with_handle()
    }

    #[test]
    fn test_run_with_default_name() {
        let (subscriber, handle) = mock_run_single_event("Hi, Youre, new world!!");

        with_default(subscriber, || {
            run("Youre", None);
        });

        handle.assert_finished();
    }

    #[test]
    fn test_run_with_custom_name() {
        let (subscriber, handle) = mock_run_single_event("Hi, Alice, new world!!");

        with_default(subscriber, || {
            run("Alice", None);
        });

        handle.assert_finished();
    }

    #[test]
    fn test_run_with_empty_name() {
        let (subscriber, handle) = mock_run_single_event("Hi, , new world!!");

        with_default(subscriber, || {
            run("", None);
        });

        handle.assert_finished();
    }

    #[test]
    fn test_run_with_japanese_name() {
        let (subscriber, handle) = mock_run_single_event("Hi, 世界, new world!!");

        with_default(subscriber, || {
            run("世界", None);
        });

        handle.assert_finished();
    }

    #[test]
    fn test_run_with_gender_man() {
        let (subscriber, handle) = mock_run_single_event("Hi, Mr. John, new world!!");

        with_default(subscriber, || {
            run("John", Some("man"));
        });

        handle.assert_finished();
    }

    #[test]
    fn test_run_with_gender_woman() {
        let (subscriber, handle) = mock_run_single_event("Hi, Ms. Alice, new world!!");

        with_default(subscriber, || {
            run("Alice", Some("woman"));
        });

        handle.assert_finished();
    }

    #[test]
    fn test_format_greeting_unknown_gender() {
        let (subscriber, handle) = subscriber::mock()
            .event(
                expect::event()
                    .with_target(env!("CARGO_PKG_NAME"))
                    .at_level(tracing::Level::ERROR),
            )
            .only()
            .run_with_handle();

        with_default(subscriber, || {
            let result = format_greeting("Unknown", Err(GreetingError::UnknownGender));
            assert_eq!(result, "Hi, Unknown");
        });

        handle.assert_finished();
    }

    #[test]
    fn test_format_greeting_invalid_gender() {
        let (subscriber, handle) = subscriber::mock()
            .event(
                expect::event()
                    .with_target(env!("CARGO_PKG_NAME"))
                    .at_level(tracing::Level::WARN),
            )
            .only()
            .run_with_handle();

        with_default(subscriber, || {
            let result = format_greeting(
                "Bob",
                Err(GreetingError::InvalidGender(String::from("other"))),
            );
            assert_eq!(result, "Hi, Bob (invalid gender: other)");
        });

        handle.assert_finished();
    }

    #[test]
    fn test_format_greeting_ok() {
        let result = format_greeting("Alice", Ok(String::from("Hi, Alice")));
        assert_eq!(result, "Hi, Alice");
    }

    #[test]
    fn test_run_with_invalid_gender() {
        let run_span = expect::span().named("run");
        let (subscriber, handle) = subscriber::mock()
            .new_span(run_span.clone())
            .enter(run_span.clone())
            .event(
                expect::event()
                    .with_target(env!("CARGO_PKG_NAME"))
                    .at_level(tracing::Level::WARN),
            )
            .event(
                expect::event()
                    .with_fields(expect::msg("Hi, Bob (invalid gender: other), new world!!")),
            )
            .exit(run_span.clone())
            .drop_span(run_span)
            .only()
            .run_with_handle();

        with_default(subscriber, || {
            run("Bob", Some("other"));
        });

        handle.assert_finished();
    }
}
