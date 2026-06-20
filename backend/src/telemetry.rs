//! Logging + optional OpenTelemetry trace export.
//!
//! By default this is just the console `tracing` logger we always had. If
//! `ZENITHAR_OTLP_ENDPOINT` is set (e.g. a bundled GreptimeDB), spans are ALSO
//! exported as OTLP/HTTP traces — a call becomes one trace, its per-participant
//! drivers nested spans, and the diagnostic events (`first RTP in`, …) ride along
//! as span events.
//!
//! Telemetry is strictly best-effort and off the critical path: export is batched
//! on a background task with a short timeout, so a down or slow collector only
//! drops batches — it never blocks a request or crashes the process. With no
//! endpoint configured there is zero overhead and no dependency on any collector.

use std::time::Duration;

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: &str = "zenithar-backend";

/// Install the global subscriber. Returns the tracer provider when OTLP export is
/// on, so `main` can flush it on shutdown; `None` means console-only logging.
pub fn init() -> Option<SdkTracerProvider> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let fmt_layer = tracing_subscriber::fmt::layer();

    let (otel_layer, provider) = match endpoint() {
        Some(ep) => match build_provider(&ep) {
            Ok(provider) => {
                global::set_tracer_provider(provider.clone());
                let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("zenithar"));
                (Some(layer), Some(provider))
            }
            // Telemetry must never fail startup — fall back to console logging.
            Err(e) => {
                eprintln!("zenithar: OTLP export disabled (build failed): {e}");
                (None, None)
            }
        },
        None => (None, None),
    };

    // `Option<Layer>` is a no-op Layer when `None`, so the same registry covers
    // both the console-only and console+OTLP cases.
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();

    if provider.is_some() {
        tracing::info!(endpoint = %endpoint().unwrap_or_default(), "OTLP trace export enabled");
    }
    provider
}

/// Best-effort flush + shutdown so buffered spans aren't lost on a clean exit.
pub fn shutdown(provider: Option<SdkTracerProvider>) {
    if let Some(p) = provider {
        let _ = p.shutdown();
    }
}

fn endpoint() -> Option<String> {
    std::env::var("ZENITHAR_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty())
}

fn build_provider(endpoint: &str) -> anyhow::Result<SdkTracerProvider> {
    // A programmatic endpoint is used verbatim by the OTLP/HTTP exporter (unlike
    // the env-var path, it does NOT append the signal path), so we add the
    // standard `/v1/traces` ourselves. This makes ZENITHAR_OTLP_ENDPOINT a base
    // URL: a plain collector (`http://host:4318`) and GreptimeDB's OTLP base
    // (`http://host:4000/v1/otlp`) both work.
    let url = if endpoint.ends_with("/v1/traces") {
        endpoint.to_string()
    } else {
        format!("{}/v1/traces", endpoint.trim_end_matches('/'))
    };
    // GreptimeDB requires these headers to accept OTLP traces (the pipeline name
    // is mandatory; the db name defaults to `public`). They're harmless to a
    // plain OTLP collector, which ignores unknown headers.
    let headers = std::collections::HashMap::from([
        (
            "x-greptime-pipeline-name".to_string(),
            "greptime_trace_v1".to_string(),
        ),
        ("x-greptime-db-name".to_string(), "public".to_string()),
    ]);
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(url)
        .with_headers(headers)
        .with_timeout(Duration::from_secs(3))
        .build()?;
    let resource = Resource::builder().with_service_name(SERVICE_NAME).build();
    Ok(SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build())
}
