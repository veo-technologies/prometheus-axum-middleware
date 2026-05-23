// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

use axum::response::IntoResponse;
use prometheus::{TextEncoder, gather};

/// Gathers metrics and encodes them as Prometheus text exposition format.
pub async fn render() -> impl IntoResponse {
    let metrics = gather();
    let encoder = TextEncoder::new();
    encoder.encode_to_string(&metrics).expect("Failed to encode metrics")
}
