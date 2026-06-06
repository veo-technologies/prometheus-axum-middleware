// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

use axum::response::IntoResponse;
use prometheus::{TextEncoder, gather};

/// This function gathers the metrics and encodes them to a string
pub async fn render() -> impl IntoResponse {
    let metrics = gather();
    let encoder = TextEncoder::new();
    encoder.encode_to_string(&metrics).expect("Failed to encode metrics")
}
