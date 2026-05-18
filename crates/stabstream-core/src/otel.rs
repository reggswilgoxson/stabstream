/// Initialise an OpenTelemetry OTLP tracer and install it as the global
/// tracing subscriber layer.
///
/// Call this **once** at program start before any spans are created.
/// Reads `OTEL_EXPORTER_OTLP_ENDPOINT` if set; falls back to `endpoint`.
///
/// # Errors
///
/// Returns an error if the OTLP pipeline cannot be built (e.g. the endpoint
/// is unreachable — the SDK will buffer and retry, so this rarely fails at
/// startup) or if a global subscriber is already installed.
pub fn install(endpoint: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::runtime::Tokio;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let resolved_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| endpoint.to_string());

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(resolved_endpoint),
        )
        .with_trace_config(
            opentelemetry_sdk::trace::config().with_resource(
                opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    "stabstream",
                )]),
            ),
        )
        .install_batch(Tokio)?;

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(otel_layer)
        .try_init()?;

    Ok(())
}

/// Shut down the global OTel tracer, flushing any buffered spans.
pub fn shutdown() {
    opentelemetry::global::shutdown_tracer_provider();
}
