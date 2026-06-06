// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

use prometheus::gather;

/// Installs a Prometheus pusher that will send metrics to the specified push gateway URL at regular intervals.
/// The `job_name` is the name of the job that will be used to identify the metrics.
/// The `push_url` is the URL of the Prometheus push gateway.
/// The `interval` is the duration between each push.
/// The `labels` are additional labels that will be added to the metrics.
/// The `http_client` is a reference to the reqwest client that will be used to send the metrics.
pub fn install_pusher(
    job_name: &str,
    push_url: &str,
    interval: std::time::Duration,
    labels: &[(&str, &str)],
    http_client: reqwest::Client,
    auth: Option<(String, String)>,
) {
    use base64::prelude::*;
    use prometheus_reqwest_remote_write::WriteRequest;
    use reqwest::header::AUTHORIZATION;
    use tracing::{debug, error, info};

    let mut labels = labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
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
            let write_request = WriteRequest::from_metric_families(metrics, Some(labels.clone())).expect("Could not create write request");
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
