// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use prometheus::{TextEncoder, gather};

/// Gathers metrics and encodes them as Prometheus text exposition format.
pub async fn render() -> Response {
    let metrics = gather();
    let encoder = TextEncoder::new();

    match encoder.encode_to_string(&metrics) {
        Ok(metrics) => metrics.into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "failed to encode metrics").into_response(),
    }
}
