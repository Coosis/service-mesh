use std::sync::OnceLock;

use opentelemetry::global::{self, BoxedTracer};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use opentelemetry_stdout::SpanExporter;

type Result<T> = std::result::Result<T, crate::error::ProxyError>;

pub fn get_tracer() -> &'static BoxedTracer {
    static TRACER: OnceLock<BoxedTracer> = OnceLock::new();
    TRACER.get_or_init(|| global::tracer("side-car-proxy"))
}

pub fn init_tracer_provider() {
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(SpanExporter::default())
        .build();
    global::set_tracer_provider(provider);
}

pub fn init_tracing_and_propagation() {
    // your init_tracer_provider();
    global::set_text_map_propagator(TraceContextPropagator::new());
}

pub fn init_tracer_exporter() -> Result<()> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
        .build()?;

    let resource = Resource::builder()
        .with_attributes(vec![
            opentelemetry::KeyValue::new("service.name", "side-car-proxy"),
            // opentelemetry::KeyValue::new("service.namespace", "edge"),
            // opentelemetry::KeyValue::new(
            //     "service.instance.id",
            //     format!("{}", std::process::id()),
            // ),
        ])
        .build();

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    global::set_tracer_provider(tracer_provider);

    Ok(())
}
