// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

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
//! prometheus-axum-middleware = "0.2.1"
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

mod metrics;
mod middleware;
#[cfg(feature = "remote-write")]
mod remote_write;
mod render;

pub use metrics::{add_excluded_paths, set_prefix};
pub use middleware::{PrometheusAxumLayer, PrometheusService};
#[cfg(feature = "remote-write")]
pub use remote_write::install_pusher;
pub use render::render;

#[cfg(test)]
mod tests;
