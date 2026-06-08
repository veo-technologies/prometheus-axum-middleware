// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

//! # prometheus-axum-middleware
//!
//! Middleware and utilities for integrating Prometheus metrics with Axum applications using the default Prometheus registry.
//!
//! This crate is different from axum-prometheus since it uses the real prometheus implementation behind the scenes,
//! so you can add your own metrics to the same registry using the built-in prometheus macros and APIs.
//! It also supports the Prometheus remote write protocol, so you can push metrics to a Prometheus Pushgateway or remote write endpoint.
//! The output is encoded using protobuf and compressed with snappy, using the `prometheus_reqwest_remote_write` crate.
//! If you do not need remote write support, build the crate without default features.
//!
//! ## Features
//!
//! - Collects HTTP request metrics (counters, histograms, gauges) for method, endpoint, status, and body sizes.
//! - Allows dynamic prefixing of metric names for multi-service environments.
//! - Supports excluding specific paths from metrics collection (e.g., health checks).
//! - Provides a `/metrics` endpoint compatible with Prometheus scraping.
//! - Includes a pusher for sending metrics to a Prometheus Pushgateway or remote write endpoint.
//!
//! ## MSRV
//!
//! The minimum supported Rust version is 1.85.
//!
//! ## Public API
//!
//! - [`set_prefix`] - Set a prefix for all HTTP metrics (should be called before the first request).
//! - [`add_excluded_paths`] - Exclude paths from metrics collection (e.g., `/healthcheck`).
//! - [`PrometheusAxumLayer`] - Axum middleware to record Prometheus metrics for each HTTP request.
//! - [`render`] - Handler for the `/metrics` endpoint, returns all metrics in Prometheus text format.
//! - `install_pusher` - Periodically push metrics to a Prometheus Pushgateway or remote write endpoint when the `remote-write` feature is enabled, returning a Tokio task handle.
//!
//! ## Usage
//!
//! Add prometheus-axum-middleware to your Cargo.toml.
//!
//! ```toml
//! [dependencies]
//! prometheus-axum-middleware = "0.3.0"
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use axum::{routing, Router};
//! use prometheus_axum_middleware::{add_excluded_paths, render, set_prefix, PrometheusAxumLayer};
//! use std::net::SocketAddr;
//!
//! #[tokio::main]
//! async fn main() {
//!     set_prefix("myservice");
//!     add_excluded_paths(&["/healthcheck"]);
//!
//!     let app = Router::new()
//!         .route("/test_body_size", routing::get(async || "Hello, World!"))
//!         .route("/metrics", routing::get(render))
//!         .layer(PrometheusAxumLayer::new());
//!
//!     let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 3000)))
//!         .await
//!         .unwrap();
//!     axum::serve(listener, app).await.unwrap()
//! }
//! ```

mod metrics;
mod middleware;
#[cfg(feature = "remote-write")]
mod pusher;
#[cfg(feature = "remote-write")]
mod remote_write;
mod render;

pub use metrics::{add_excluded_paths, set_prefix};
pub use middleware::{PrometheusAxumLayer, PrometheusService};
#[cfg(feature = "remote-write")]
pub use pusher::install_pusher;
pub use render::render;

#[cfg(test)]
mod tests;
