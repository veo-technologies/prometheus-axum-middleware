//! # prometheus-axum-middleware
//!
//! Middleware and utilities for integrating Prometheus metrics with Axum applications using the default Prometheus registry.
//!
//! This crate is different from axum-prometheus since it uses directy the real prometheus implementation behind the scenes,
//! so you will be easily able to add your own metrics to the same registry using the builtin prometheus macros / apis.
//! It also supports the prometheus remote write protocol, so you can push metrics to a Prometheus Pushgateway or remote write endpoint.
//! The output is encoded using protobuffers and compressed with snappy, using the builtin `prometheus_reqwest_remote_write` crate.
//! If you do not need the remote write support you can build the crate without the default features
//!
//! ## Features
//! - Collects HTTP request metrics (counters, histograms, gauges) for method, endpoint, status and body sizes.
//! - Allows dynamic prefixing of metric names for multi-service environments.
//! - Supports excluding specific paths from metrics collection (e.g., health checks).
//! - Provides a `/metrics` endpoint compatible with Prometheus scraping.
//! - Includes a pusher for sending metrics to a Prometheus Pushgateway or remote write endpoint (feature).
//!
//! ### Renaming Metrics
//!
//! These metrics can be renamed by specifying environmental variables in your environment or by using the `set_prefix` function before
//! starting using them:
//! - `AXUM_HTTP_REQUESTS_TOTAL`
//! - `AXUM_HTTP_REQUESTS_DURATION_SECONDS`
//! - `AXUM_HTTP_REQUESTS_PENDING`
//! - `AXUM_HTTP_RESPONSE_BODY_SIZE` (if body size tracking is enabled)
//! 
//! ## Public API
//!
//! - [`set_prefix`] - Set a prefix for all HTTP metrics (should be called before the first request).
//! - [`add_excluded_paths`] - Exclude a slice of paths from metrics collection (e.g., `/healthcheck`).
//! - [`PrometheusAxumLayer`] - Axum middleware to record Prometheus metrics for each HTTP request.
//! - [`render`] - Handler for the `/metrics` endpoint, returns all metrics in Prometheus text format.
//! - [`install_pusher`] - Periodically push metrics to a Prometheus Pushgateway or remote write endpoint.
//!
//! ## Usage
//! 
//! Add prometheus-axum-middleware to your Cargo.toml.
//! 
//! ```toml
//! [dependencies]
//! prometheus-axum-middleware = "0.1.0"
//! ```
//! 
//! ## Example
//! ```rust,no_run
//! use prometheus_axum_middleware::{set_prefix, add_excluded_paths, PrometheusAxumLayer, render};
//! use axum::{Router, routing};
//! use std::net::SocketAddr;
//! 
//! #[tokio::main]
//! async fn main() {
//!     // Set up metrics before starting your Axum app
//!     set_prefix("myservice");
//!     add_excluded_paths(&["/healthcheck"]);
//! 
//!     let app = Router::new()
//!            .route("/test_body_size", routing::get(async || "Hello, World!"))
//!            .route("/metrics", routing::get(render))
//!            .layer(PrometheusAxumLayer::new());
//!
//!     // Optionally, call `install_pusher` to push metrics to a remote endpoint
//! 
//!     let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 3000)))
//!         .await
//!         .unwrap();
//!     axum::serve(listener, app).await.unwrap()
//! }
//! ```
//! 
//! ## Prometheus push gateway feature
//! 
//! This feature allows you to push metrics to a Prometheus Pushgateway using the remote write endpoint. 
//! The data sent to this endpoint is encoded using protobuffers and compressed with snappy, using the builtin `prometheus_reqwest_remote_write` crate.
//! This feature is enabled by default and bring in several dependencies (reqwest, tracing, base64...), if you do not need the remote write support you can build the crate 
//! without the default features.

use axum::{
    extract::{MatchedPath, Request},
    response::{IntoResponse, Response},
    body::HttpBody,
};
use prometheus::{
    GaugeVec, HistogramVec, IntCounterVec, TextEncoder, gather, register_gauge_vec, register_histogram_vec,
    register_int_counter_vec,
};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Instant;
use std::vec;
use tower::{Layer, Service};
use std::task::{Context, Poll};
use std::future::Future;
use std::pin::Pin;

static PREFIXED_HTTP_REQUESTS_TOTAL: OnceLock<String> = OnceLock::new();
static PREFIXED_HTTP_REQUESTS_DURATION_SECONDS: OnceLock<String> = OnceLock::new();
static PREFIXED_HTTP_RESPONSE_BODY_SIZE: OnceLock<String> = OnceLock::new();
static PREFIXED_HTTP_REQUESTS_PENDING: OnceLock<String> = OnceLock::new();

static HTTP_REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        get_http_requests_total(),
        "Total number of HTTP requests",
        &["method", "endpoint", "status"]
    )
    .unwrap()
});
static HTTP_REQUEST_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        get_http_requests_duration_seconds(),
        "HTTP request latencies in seconds",
        &["method", "endpoint", "status"]
    )
    .unwrap()
});

static HTTP_RESPONSE_BODY_SIZE: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        get_response_body_size(),
        "Size of HTTP response bodies in bytes",
        &["method", "endpoint"]
    )
    .unwrap()
});
static HTTP_REQUESTS_PENDING: LazyLock<GaugeVec> = LazyLock::new(|| {
    register_gauge_vec!(
        get_http_requests_pending(),
        "Number of pending HTTP requests",
        &["method", "endpoint"]
    )
    .unwrap()
});
fn get_http_requests_total() -> &'static str {
    let env_value =
        std::env::var("AXUM_HTTP_REQUESTS_TOTAL").unwrap_or_else(|_| "axum_http_requests_total".to_string());
    PREFIXED_HTTP_REQUESTS_TOTAL.get_or_init(|| env_value)
}
fn get_response_body_size() -> &'static str {
    let env_value =
        std::env::var("AXUM_HTTP_RESPONSE_BODY_SIZE").unwrap_or_else(|_| "axum_http_response_body_size".to_string());
    PREFIXED_HTTP_RESPONSE_BODY_SIZE.get_or_init(|| env_value)
}
fn get_http_requests_pending() -> &'static str {
    let env_value =
        std::env::var("AXUM_HTTP_REQUESTS_PENDING").unwrap_or_else(|_| "axum_http_requests_pending".to_string());
    PREFIXED_HTTP_REQUESTS_PENDING.get_or_init(|| env_value)
}
fn get_http_requests_duration_seconds() -> &'static str {
    let env_value = std::env::var("AXUM_HTTP_REQUESTS_DURATION_SECONDS")
        .unwrap_or_else(|_| "axum_http_requests_duration_seconds".to_string());
    PREFIXED_HTTP_REQUESTS_DURATION_SECONDS.get_or_init(|| env_value)
}

static EXCLUDED_PATHS: LazyLock<Mutex<Vec<&'static str>>> = LazyLock::new(|| Mutex::new(vec!["/metrics"]));

fn excluded_path(path: &str) -> bool {
    EXCLUDED_PATHS
        .lock()
        .expect("Failed to lock EXCLUDED_PATHS")
        .iter()
        .any(|&p| path.starts_with(p))
}

/// Sets a prefix for the HTTP request metrics.
/// This is useful for namespacing metrics in environments where multiple applications
/// NOTE: this should be called before the first request is processed, otherwise it will not take effect.
pub fn set_prefix(prefix: &str) {
    PREFIXED_HTTP_REQUESTS_TOTAL.get_or_init(|| format!("{}_http_requests_total", prefix));
    PREFIXED_HTTP_REQUESTS_DURATION_SECONDS.get_or_init(|| format!("{}_http_requests_duration_seconds", prefix));
    PREFIXED_HTTP_REQUESTS_PENDING.get_or_init(|| format!("{}_http_requests_pending", prefix));
    PREFIXED_HTTP_RESPONSE_BODY_SIZE.get_or_init(|| format!("{}_http_response_body_size", prefix));
}

/// Adds one or more paths to the list of excluded paths for metrics collection, every url that starts with one
/// of the paths in the list is excluded.
/// This is useful for paths that you do not want to track metrics for, such as health checks or static assets,
/// NOTE: the /metrics endpoint, used by prometheus to scrape the service is in the list by default.
pub fn add_excluded_paths(paths: &[&'static str]) {
    EXCLUDED_PATHS
        .lock()
        .expect("Failed to lock EXCLUDED_PATHS")
        .extend_from_slice(paths);
}

#[derive(Clone)]
pub struct PrometheusAxumLayer;

impl PrometheusAxumLayer {
    pub fn new() -> Self {
        Self
    }
}
impl Default for PrometheusAxumLayer {
    fn default() -> Self {
        Self::new()
    }
}
impl<S> Layer<S> for PrometheusAxumLayer {
    type Service = PrometheusService<S>;

    fn layer(&self, service: S) -> Self::Service {
        PrometheusService { service }
    }
}

#[derive(Clone)]
pub struct PrometheusService<S> {
    service: S,
}

impl<S, B> Service<Request<B>> for PrometheusService<S>
where
    S: Service<Request<B>, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let method = req.method().as_str().to_owned();
        let path = req
            .extensions()
            .get::<MatchedPath>()
            .map(|p| p.as_str().to_owned())
            .unwrap_or_else(|| req.uri().path().to_owned());

        let skip = excluded_path(&path);
        if !skip {
            HTTP_REQUESTS_PENDING.with_label_values(&[&method, &path]).inc();
        }
        let start = Instant::now();

        let mut service = self.service.clone();
        Box::pin(async move {
            let response = service.call(req).await?;
            let status = response.status().as_u16().to_string();

            if !skip {
                HTTP_REQUESTS_PENDING.with_label_values(&[&method, &path]).dec();
                HTTP_REQUESTS_TOTAL.with_label_values(&[&method, &path, &status]).inc();

                let elapsed = start.elapsed().as_secs_f64();
                HTTP_REQUEST_DURATION_SECONDS
                    .with_label_values(&[&method, &path, &status])
                    .observe(elapsed);

                let size = response.body().size_hint().lower();
                HTTP_RESPONSE_BODY_SIZE
                    .with_label_values(&[&method, &path])
                    .observe(size as f64);
            }

            Ok(response)
        })
    }
}

/// This function gathers the metrics and encodes them to a string
pub async fn render() -> impl IntoResponse {
    let metrics = gather();
    let encoder = TextEncoder::new();
    encoder.encode_to_string(&metrics).expect("Failed to encode metrics")
}

/// Installs a Prometheus pusher that will send metrics to the specified push gateway URL at regular intervals.
/// The `job_name` is the name of the job that will be used to identify the metrics.
/// The `push_url` is the URL of the Prometheus push gateway.
/// The `interval` is the duration between each push.
/// The `labels` are additional labels that will be added to the metrics.
/// The `http_client` is a reference to the reqwest client that will be used to send the metrics.
#[cfg(feature = "remote-write")]
pub fn install_pusher(
    job_name: &str,
    push_url: &str,
    interval: std::time::Duration,
    labels: &[(&str, &str)],
    http_client: reqwest::Client,
    auth: Option<(String, String)>,
) {
    use base64::prelude::*;
    use reqwest::header::AUTHORIZATION;
    use prometheus_reqwest_remote_write::WriteRequest;
    use tracing::{debug, error, info};

    let mut labels = labels
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect::<Vec<_>>();
    labels.push((String::from("job"), job_name.to_string()));
    let push_url = push_url.to_owned();
    let (username, token) = auth.unwrap_or_default();
    let user_agent = job_name.to_string();
    tokio::spawn(async move {
        info!("Installed Prometheus recorder with push gateway at {push_url}");
        loop {
            tokio::time::sleep(interval).await; // Run every X seconds 
            let metrics = gather();
            let metrics_len = metrics.len();
            let write_request = WriteRequest::from_metric_families(metrics, Some(labels.clone()))
                .expect("Could not create write request");
            let mut http_request = write_request
                .build_http_request(http_client.clone(), &push_url, &user_agent)
                .expect("Could not build http request");
            if !username.is_empty() && !token.is_empty() {
                http_request.headers_mut().insert(
                    AUTHORIZATION,
                    format!("Basic {}", BASE64_STANDARD.encode(format!("{username}:{token}")))
                        .parse()
                        .unwrap(),
                );
            }
            match http_client.execute(http_request).await {
                Ok(r) => {
                    if r.status().is_success() {
                        debug!("Metrics for {metrics_len} families sent successfully");
                    } else {
                        error!(
                            "Failed to send metrics: {:?}",
                            r.text().await.expect("Could not read body from response")
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to send metrics: {:?}", e);
                }
            }
        }
    });
}

/// Note: the tests run in parallel so every test can not assume the prefix of the metrics
/// or the value of the metrics used in the other tests.
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing, Router};
    use tower::ServiceExt;

    #[test]
    fn test_set_prefix() {
        // we test on body size since it's unused in the middleware at the moment
        // and we do not risk the test to fail if multiple tests run in parallel
        set_prefix("test_prefix");
        assert_eq!(
            get_response_body_size(),
            "test_prefix_http_response_body_size"
        );
    }

    #[tokio::test]
    async fn test_metrics_layer_basic() {
        let app = Router::new()
            .route("/test", routing::get(async || "Hello, World!"))
            .layer(PrometheusAxumLayer::new());

        // we do not know the initial value of the counter since we may use it in multiple tests
        let counter = HTTP_REQUESTS_TOTAL.get_metric_with_label_values(&["GET", "/test", "200"]).unwrap().get();
        let another_counter = HTTP_REQUESTS_TOTAL.get_metric_with_label_values(&["GET", "/test2", "200"]).unwrap().get();
        assert_eq!(another_counter, 0);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);

        let updated_counter = HTTP_REQUESTS_TOTAL.get_metric_with_label_values(&["GET", "/test", "200"]).unwrap().get();
        let another_counter = HTTP_REQUESTS_TOTAL.get_metric_with_label_values(&["GET", "/test2", "200"]).unwrap().get();
        assert_eq!(another_counter, 0);
        assert_eq!(updated_counter, counter + 1);
    }

    #[test]
    fn test_excluded_path() {
        let paths = vec!["/healthcheck"];
        add_excluded_paths(&paths);
        assert!(excluded_path("/metrics"));
        assert!(excluded_path("/healthcheck"));
        assert!(!excluded_path("/test"));
        assert!(!excluded_path("/api/v1/resource"));
    }

     #[tokio::test]
    async fn test_metrics_layer_body_size() {
        let app = Router::new()
            .route("/test_body_size", routing::get(async || "Hello, World!"))
            .layer(PrometheusAxumLayer::new());

        // we do not know the initial value of the counter since we may use it in multiple tests
        let counter = HTTP_REQUESTS_TOTAL.get_metric_with_label_values(&["GET", "/test_body_size", "200"]).unwrap().get();
        assert_eq!(counter, 0);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/test_body_size")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);

        let updated_counter = HTTP_REQUESTS_TOTAL.get_metric_with_label_values(&["GET", "/test_body_size", "200"]).unwrap().get();
        assert_eq!(updated_counter, counter + 1);
        let body_size = HTTP_RESPONSE_BODY_SIZE
            .get_metric_with_label_values(&["GET", "/test_body_size"])
            .unwrap()
            .get_sample_sum();
        assert_eq!(body_size, 13.0, "it should be 13 bytes for \"Hello, World!\"");
    }

    async fn call_metrics(app: Router) -> String {
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), i32::MAX as usize).await.expect("Body should be there");
        String::from_utf8(body.to_vec()).expect("Response should be valid UTF-8")
    }

    #[tokio::test]
    async fn test_render_and_path_skipped() {
        let app = Router::new()
            .route("/test_new", routing::get(async || "Hello, World!"))
            .route("/metrics", routing::get(render))
            .layer(PrometheusAxumLayer::new());

        let body_str = call_metrics(app.clone()).await;
        assert!(!body_str.contains("endpoint=\"/metrics\""));
        assert!(!body_str.contains("endpoint=\"/test_new\""));

        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/test_new")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body_str = call_metrics(app.clone()).await;
        assert!(body_str.contains("http_requests_duration_seconds_bucket"));
        assert!(body_str.contains("response_body_size_bucket"));
        assert!(body_str.contains("endpoint=\"/test_new\""));
        assert!(body_str.contains("# TYPE "));
    }

    #[cfg(feature = "remote-write")]
    #[tokio::test]
    async fn test_install_pusher() {
        use reqwest::header::{CONTENT_ENCODING, CONTENT_TYPE, USER_AGENT, AUTHORIZATION};
        use std::{net::SocketAddr, sync::Arc};
        use axum::body::to_bytes;

        let job_name = "test_job";
        let interval = std::time::Duration::from_secs(1);
        let labels = &[("label1", "value1")];
        let http_client = reqwest::Client::new();

        // Shared state to capture the request body
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let captured_headers = Arc::new(Mutex::new(Vec::new()));
        let captured_headers_clone = captured_headers.clone();

        // build a simple service to receive the pushed metrics
        let app = Router::new()
            .route(
                "/push",
                axum::routing::post({
                    move |req: Request<Body>| {
                        let captured = captured_clone.clone();
                        let captured_headers = captured_headers_clone.clone();
                        async move {
                            // Capture headers
                            let headers = req.headers().clone();
                            let (_, body) = req.into_parts();
                            captured_headers.lock().unwrap().push(headers);
                            let bytes = to_bytes(body, i32::MAX as usize).await.unwrap();

                            // Capture body
                            captured.lock().unwrap().push(bytes);
                            Ok::<_, std::convert::Infallible>("ok")
                        }
                    }
                })
            );
        // Bind to a random port
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        // Run the server in the background
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async { shutdown_rx.await.ok(); })
                .await
                .unwrap();
        });

        // run the pusher
        install_pusher(job_name, &format!("http://{local_addr}/push"), interval, labels, http_client, Some((String::from("user"), String::from("password"))));

        // The pusher runs in a separate task, so we can't assert anything here.
        // Just ensure it doesn't panic.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Shutdown the server
        let _ = shutdown_tx.send(());

        // Assert that we captured at least one request
        let captured = captured.lock().unwrap();
        assert!(!captured.is_empty(), "No metrics were pushed");

        let headers = captured_headers.lock().unwrap()[0].clone();
        assert!(headers.iter().any(|(name, value)| name.as_str() == AUTHORIZATION && value.to_str().unwrap().starts_with("Basic ")));
        assert!(headers.iter().any(|(name, value)| name.as_str() == CONTENT_ENCODING && value.to_str().unwrap() == "snappy"));
        assert!(headers.iter().any(|(name, value)| name.as_str() == USER_AGENT && value.to_str().unwrap() == job_name));
        assert!(headers.iter().any(|(name, value)| name.as_str() == CONTENT_TYPE && value.to_str().unwrap() == "application/x-protobuf"));
    }
}