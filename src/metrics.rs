use prometheus::{
    GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, register_gauge_vec, register_histogram_vec, register_int_counter_vec,
};
use std::sync::{LazyLock, Mutex, OnceLock};

pub(crate) static PREFIXED_HTTP_REQUESTS_TOTAL: OnceLock<String> = OnceLock::new();
pub(crate) static PREFIXED_HTTP_REQUESTS_DURATION_SECONDS: OnceLock<String> = OnceLock::new();
pub(crate) static PREFIXED_HTTP_RESPONSE_BODY_SIZE: OnceLock<String> = OnceLock::new();
pub(crate) static PREFIXED_HTTP_REQUESTS_PENDING: OnceLock<String> = OnceLock::new();

pub(crate) static HTTP_REQUEST_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        get_http_requests_duration_seconds(),
        "HTTP request latencies in seconds",
        &["method", "endpoint", "status"]
    )
    .unwrap()
});

pub(crate) static HTTP_REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        get_http_requests_total(),
        "Total number of HTTP requests",
        &["method", "endpoint", "status"]
    )
    .unwrap()
});

static BODY_SIZE_BUCKETS: &[f64] = &[
    100.0,
    500.0,
    1_000.0,
    5_000.0,
    10_000.0,
    50_000.0,
    100_000.0,
    500_000.0,
    1_000_000.0,
    5_000_000.0,
];

pub(crate) static HTTP_RESPONSE_BODY_SIZE: LazyLock<HistogramVec> = LazyLock::new(|| {
    let opts = HistogramOpts::new(get_response_body_size(), "Size of HTTP response bodies in bytes").buckets(BODY_SIZE_BUCKETS.to_vec());
    HistogramVec::new(opts, &["method", "endpoint"])
        .and_then(|h| {
            prometheus::register(Box::new(h.clone()))?;
            Ok(h)
        })
        .unwrap()
});
pub(crate) static HTTP_REQUESTS_PENDING: LazyLock<GaugeVec> = LazyLock::new(|| {
    register_gauge_vec!(
        get_http_requests_pending(),
        "Number of pending HTTP requests",
        &["method", "endpoint"]
    )
    .unwrap()
});
fn get_http_requests_total() -> &'static str {
    let env_value = std::env::var("AXUM_HTTP_REQUESTS_TOTAL").unwrap_or_else(|_| "axum_http_requests_total".to_string());
    PREFIXED_HTTP_REQUESTS_TOTAL.get_or_init(|| env_value)
}
pub(crate) fn get_response_body_size() -> &'static str {
    let env_value = std::env::var("AXUM_HTTP_RESPONSE_BODY_SIZE").unwrap_or_else(|_| "axum_http_response_body_size".to_string());
    PREFIXED_HTTP_RESPONSE_BODY_SIZE.get_or_init(|| env_value)
}
fn get_http_requests_pending() -> &'static str {
    let env_value = std::env::var("AXUM_HTTP_REQUESTS_PENDING").unwrap_or_else(|_| "axum_http_requests_pending".to_string());
    PREFIXED_HTTP_REQUESTS_PENDING.get_or_init(|| env_value)
}
fn get_http_requests_duration_seconds() -> &'static str {
    let env_value =
        std::env::var("AXUM_HTTP_REQUESTS_DURATION_SECONDS").unwrap_or_else(|_| "axum_http_requests_duration_seconds".to_string());
    PREFIXED_HTTP_REQUESTS_DURATION_SECONDS.get_or_init(|| env_value)
}

static EXCLUDED_PATHS: LazyLock<Mutex<Vec<&'static str>>> = LazyLock::new(|| Mutex::new(vec!["/metrics"]));

pub(crate) fn excluded_path(path: &str) -> bool {
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
