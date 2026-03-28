//! Telemetry initialization: tracing subscriber with optional `OTel` export.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Guard that shuts down `OTel` providers on drop.
#[must_use]
pub struct TelemetryGuard {
    #[cfg(feature = "otel")]
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        #[cfg(feature = "otel")]
        if let Some(provider) = self.tracer_provider.take()
            && let Err(e) = provider.shutdown()
        {
            tracing::error!("OTel tracer shutdown error: {e}");
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
        let (otel_layer, provider) = init_otel();

        tracing_subscriber::registry()
            .with(otel_layer)
            .with(env_filter)
            .with(fmt_layer)
            .init();

        TelemetryGuard {
            tracer_provider: provider,
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

/// Build `OTel` layer and provider, activated only when endpoint is configured.
#[cfg(feature = "otel")]
fn init_otel() -> (
    Option<
        tracing_opentelemetry::OpenTelemetryLayer<
            tracing_subscriber::Registry,
            opentelemetry_sdk::trace::Tracer,
        >,
    >,
    Option<opentelemetry_sdk::trace::SdkTracerProvider>,
) {
    use opentelemetry::KeyValue;
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_otlp::SpanExporter;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_opentelemetry::OpenTelemetryLayer;

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|ep| !ep.is_empty());

    let Some(_endpoint) = endpoint else {
        return (None, None);
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

    let exporter = match SpanExporter::builder().with_http().build() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to create OTLP exporter, running without OTel: {e}");
            return (None, None);
        }
    };

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let layer = OpenTelemetryLayer::new(provider.tracer(env!("CARGO_PKG_NAME")));

    (Some(layer), Some(provider))
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "otel")]
    fn init_otel_returns_none_when_endpoint_empty() {
        // OTEL_EXPORTER_OTLP_ENDPOINT is set to "" in test env (mise.toml)
        // which is filtered out by .filter(|ep| !ep.is_empty())
        let (layer, provider) = super::init_otel();
        assert!(layer.is_none());
        assert!(provider.is_none());
    }

    #[test]
    fn telemetry_guard_drop_is_safe() {
        // Guard with no provider should drop without panic
        let guard = super::TelemetryGuard {
            #[cfg(feature = "otel")]
            tracer_provider: None,
        };
        drop(guard);
    }
}
