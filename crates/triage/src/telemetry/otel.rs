//! Dormant OTLP trace exporter. Only built when `Config::otel_endpoint` is set.
//!
//! The opentelemetry crates bump together roughly quarterly; keep every use of them in this file.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter, TracerProviderBuilder};
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;
use url::Url;

const SERVICE_NAME: &str = "triage";

/// Flushes and stops the batch exporter on drop.
#[derive(Debug)]
pub struct Otel {
    provider: SdkTracerProvider,
}

impl Drop for Otel {
    fn drop(&mut self) {
        // An unreachable collector already warned from `WarnOnce`; nothing useful to add here.
        let _ = self.provider.shutdown();
    }
}

/// The OTLP layer and the handle that flushes it. `None` endpoint builds nothing.
pub fn layer<S>(
    endpoint: Option<&Url>,
    warnings: &mut Vec<String>,
) -> Option<(impl Layer<S> + use<S>, Otel)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let endpoint = endpoint?;
    let exporter = match exporter(endpoint) {
        Ok(exporter) => exporter,
        Err(err) => {
            warnings.push(format!(
                "cannot build OTLP exporter for {endpoint}: {err}; traces stay local"
            ));
            return None;
        }
    };
    let provider = provider_builder()
        .with_batch_exporter(WarnOnce::new(exporter, endpoint))
        .build();
    let tracer = provider.tracer(SERVICE_NAME);
    Some((
        tracing_opentelemetry::layer().with_tracer(tracer),
        Otel { provider },
    ))
}

fn exporter(endpoint: &Url) -> Result<opentelemetry_otlp::SpanExporter, String> {
    opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(traces_url(endpoint).map_err(|e| e.to_string())?)
        .with_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())
}

/// The base endpoint plus `v1/traces`, matching how `OTEL_EXPORTER_OTLP_ENDPOINT` is read.
/// A programmatic endpoint is otherwise used verbatim by the exporter.
fn traces_url(base: &Url) -> Result<Url, url::ParseError> {
    let mut base = base.clone();
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join("v1/traces")
}

fn provider_builder() -> TracerProviderBuilder {
    let resource = Resource::builder()
        .with_service_name(SERVICE_NAME)
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
        .build();
    SdkTracerProvider::builder().with_resource(resource)
}

/// Logs the first failed export and stays quiet after, so a missing collector costs one line.
struct WarnOnce<E> {
    inner: E,
    endpoint: String,
    warned: AtomicBool,
}

impl<E> WarnOnce<E> {
    fn new(inner: E, endpoint: &Url) -> Self {
        Self {
            inner,
            endpoint: endpoint.to_string(),
            warned: AtomicBool::new(false),
        }
    }
}

impl<E> fmt::Debug for WarnOnce<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WarnOnce")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl<E: SpanExporter> SpanExporter for WarnOnce<E> {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        let result = self.inner.export(batch).await;
        if let Err(err) = &result
            && !self.warned.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                endpoint = %self.endpoint,
                error = %err,
                "cannot export traces; further OTLP errors are silent"
            );
        }
        result
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use opentelemetry::trace::{Span as _, Tracer as _};
    use tracing_subscriber::Registry;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| io::Error::other("poisoned"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn no_endpoint_builds_no_layer() {
        let mut warnings = Vec::new();
        assert!(layer::<Registry>(None, &mut warnings).is_none());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn endpoint_builds_layer_without_connecting() -> TestResult {
        let mut warnings = Vec::new();
        let url = Url::parse("http://127.0.0.1:1")?;
        assert!(layer::<Registry>(Some(&url), &mut warnings).is_some());
        assert!(warnings.is_empty(), "{warnings:?}");
        Ok(())
    }

    #[test]
    fn traces_path_is_appended_to_base() -> TestResult {
        for (base, want) in [
            ("http://h:4318", "http://h:4318/v1/traces"),
            ("http://h:4318/otlp", "http://h:4318/otlp/v1/traces"),
            ("http://h:4318/otlp/", "http://h:4318/otlp/v1/traces"),
        ] {
            assert_eq!(traces_url(&Url::parse(base)?)?.as_str(), want);
        }
        Ok(())
    }

    #[test]
    fn closed_port_warns_once() -> TestResult {
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let url = Url::parse(&format!("http://127.0.0.1:{port}"))?;
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || -> TestResult {
            // Simple, not batch: export runs on this thread, where the capture subscriber is set.
            let provider = provider_builder()
                .with_simple_exporter(WarnOnce::new(exporter(&url)?, &url))
                .build();
            let tracer = provider.tracer("test");
            for name in ["first", "second", "third"] {
                tracer.start(name).end();
            }
            Ok(())
        })?;
        let out = String::from_utf8(capture.0.lock().map_err(|_| "poisoned")?.clone())?;
        assert_eq!(out.matches("cannot export traces").count(), 1, "{out}");
        assert!(out.contains(&port.to_string()), "{out}");
        Ok(())
    }
}
