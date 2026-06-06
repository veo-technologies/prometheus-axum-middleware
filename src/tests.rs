// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::metrics::{HTTP_REQUESTS_PENDING, HTTP_REQUESTS_TOTAL, HTTP_RESPONSE_BODY_SIZE, excluded_path, get_response_body_size};
use crate::remote_write::install_pusher;
use crate::{PrometheusAxumLayer, add_excluded_paths, render, set_prefix};

/// Note: the tests run in parallel so every test can not assume the prefix of the metrics
/// or the value of the metrics used in the other tests.
use axum::http::Request;
use axum::{Router, body::Body, routing};
use std::sync::Mutex;
use tower::ServiceExt;

#[test]
#[ignore = "set_prefix uses OnceLock internally, so parallel tests that trigger the middleware can initialize the metric names with defaults before set_prefix runs, making the assertion non-deterministic"]
fn test_set_prefix() {
    set_prefix("test_prefix");
    assert_eq!(get_response_body_size(), "test_prefix_http_response_body_size");
}

#[tokio::test]
async fn test_metrics_layer_basic() {
    let app = Router::new()
        .route("/test", routing::get(async || "Hello, World!"))
        .layer(PrometheusAxumLayer::new());

    // we do not know the initial value of the counter since we may use it in multiple tests
    let counter = HTTP_REQUESTS_TOTAL
        .get_metric_with_label_values(&["GET", "/test", "200"])
        .unwrap()
        .get();
    let another_counter = HTTP_REQUESTS_TOTAL
        .get_metric_with_label_values(&["GET", "/test2", "200"])
        .unwrap()
        .get();
    assert_eq!(another_counter, 0);
    let response = app
        .oneshot(Request::builder().method("GET").uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let updated_counter = HTTP_REQUESTS_TOTAL
        .get_metric_with_label_values(&["GET", "/test", "200"])
        .unwrap()
        .get();
    let another_counter = HTTP_REQUESTS_TOTAL
        .get_metric_with_label_values(&["GET", "/test2", "200"])
        .unwrap()
        .get();
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
    let counter = HTTP_REQUESTS_TOTAL
        .get_metric_with_label_values(&["GET", "/test_body_size", "200"])
        .unwrap()
        .get();
    assert_eq!(counter, 0);
    let response = app
        .oneshot(Request::builder().method("GET").uri("/test_body_size").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let updated_counter = HTTP_REQUESTS_TOTAL
        .get_metric_with_label_values(&["GET", "/test_body_size", "200"])
        .unwrap()
        .get();
    assert_eq!(updated_counter, counter + 1);
    let body_size = HTTP_RESPONSE_BODY_SIZE
        .get_metric_with_label_values(&["GET", "/test_body_size"])
        .unwrap()
        .get_sample_sum();
    assert_eq!(body_size, 13.0, "it should be 13 bytes for \"Hello, World!\"");
}

async fn call_metrics(app: Router) -> String {
    let response = app
        .oneshot(Request::builder().method("GET").uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), i32::MAX as usize)
        .await
        .expect("Body should be there");
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

    let response = app
        .clone()
        .oneshot(Request::builder().method("GET").uri("/test_new").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body_str = call_metrics(app.clone()).await;
    assert!(body_str.contains("http_requests_duration_seconds_bucket"));
    assert!(body_str.contains("response_body_size_bucket"));
    assert!(body_str.contains("endpoint=\"/test_new\""));
    assert!(body_str.contains("# TYPE "));
}

#[tokio::test]
async fn test_pending_requests_decremented_after_completion() {
    let app = Router::new()
        .route("/test_pending", routing::get(async || "ok"))
        .layer(PrometheusAxumLayer::new());

    let before = HTTP_REQUESTS_PENDING
        .get_metric_with_label_values(&["GET", "/test_pending"])
        .unwrap()
        .get();

    let response = app
        .oneshot(Request::builder().method("GET").uri("/test_pending").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let after = HTTP_REQUESTS_PENDING
        .get_metric_with_label_values(&["GET", "/test_pending"])
        .unwrap()
        .get();
    assert_eq!(
        before, after,
        "pending gauge should return to its original value after request completes"
    );
}

#[cfg(feature = "remote-write")]
#[tokio::test]
async fn test_install_pusher() {
    use axum::body::to_bytes;
    use reqwest::header::{AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE, USER_AGENT};
    use std::{net::SocketAddr, sync::Arc};

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
    let app = Router::new().route(
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
        }),
    );
    // Bind to a random port
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let local_addr = listener.local_addr().unwrap();

    // Run the server in the background
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .unwrap();
    });

    // run the pusher
    install_pusher(
        job_name,
        &format!("http://{local_addr}/push"),
        interval,
        labels,
        http_client,
        Some((String::from("user"), String::from("password"))),
    );

    // The pusher runs in a separate task, so we can't assert anything here.
    // Just ensure it doesn't panic.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Shutdown the server
    let _ = shutdown_tx.send(());

    // Assert that we captured at least one request
    let captured = captured.lock().unwrap();
    assert!(!captured.is_empty(), "No metrics were pushed");

    let headers = captured_headers.lock().unwrap()[0].clone();
    assert!(
        headers
            .iter()
            .any(|(name, value)| name.as_str() == AUTHORIZATION && value.to_str().unwrap().starts_with("Basic "))
    );
    assert!(
        headers
            .iter()
            .any(|(name, value)| name.as_str() == CONTENT_ENCODING && value.to_str().unwrap() == "snappy")
    );
    assert!(
        headers
            .iter()
            .any(|(name, value)| name.as_str() == USER_AGENT && value.to_str().unwrap() == job_name)
    );
    assert!(
        headers
            .iter()
            .any(|(name, value)| name.as_str() == CONTENT_TYPE && value.to_str().unwrap() == "application/x-protobuf")
    );
}
