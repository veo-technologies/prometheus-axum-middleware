// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

use prometheus::gather;
use tokio::{runtime::TryCurrentError, task::JoinHandle};

/// Installs a Prometheus pusher that will send metrics to the specified push gateway URL at regular intervals.
///
/// The `http_client` must be a `reqwest::Client` from reqwest v0.13.
///
/// This must be called from a running Tokio runtime. The returned handle can be
/// aborted or awaited by callers that need to manage pusher shutdown.
pub fn install_pusher(
    job_name: &str,
    push_url: &str,
    interval: std::time::Duration,
    labels: &[(&str, &str)],
    http_client: reqwest::Client,
    auth: Option<(String, String)>,
) -> Result<JoinHandle<()>, TryCurrentError> {
    use base64::prelude::*;
    use prometheus_reqwest_remote_write::WriteRequest;
    use reqwest::header::{AUTHORIZATION, HeaderValue};
    use tracing::{debug, error, info};

    let mut labels = labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
    labels.push((String::from("job"), job_name.to_string()));
    let push_url = push_url.to_owned();
    let (username, token) = auth.unwrap_or_default();
    let authorization = if username.is_empty() || token.is_empty() {
        None
    } else {
        Some(format!("Basic {}", BASE64_STANDARD.encode(format!("{username}:{token}"))))
    };
    let user_agent = job_name.to_string();
    let runtime = tokio::runtime::Handle::try_current()?;

    Ok(runtime.spawn(async move {
        info!("Installed Prometheus recorder with push gateway at {push_url}");
        loop {
            tokio::time::sleep(interval).await;
            let metrics = gather();
            let metrics_len = metrics.len();
            let write_request = match WriteRequest::from_metric_families(metrics, Some(labels.clone())) {
                Ok(write_request) => write_request,
                Err(error) => {
                    error!("Could not create write request: {:?}", error);
                    continue;
                }
            };
            let mut http_request = match write_request.build_http_request(http_client.clone(), &push_url, &user_agent) {
                Ok(http_request) => http_request,
                Err(error) => {
                    error!("Could not build http request: {:?}", error);
                    continue;
                }
            };
            if let Some(authorization) = &authorization {
                match HeaderValue::from_str(authorization) {
                    Ok(authorization) => {
                        http_request.headers_mut().insert(AUTHORIZATION, authorization);
                    }
                    Err(error) => {
                        error!("Could not build authorization header: {:?}", error);
                        continue;
                    }
                }
            }
            match http_client.execute(http_request).await {
                Ok(r) => {
                    if r.status().is_success() {
                        debug!("Metrics for {metrics_len} families sent successfully");
                    } else {
                        let status = r.status();
                        let body = r
                            .text()
                            .await
                            .unwrap_or_else(|error| format!("<failed to read response body: {error}>"));
                        error!("Failed to send metrics with status {status}: {body}");
                    }
                }
                Err(e) => {
                    error!("Failed to send metrics: {:?}", e);
                }
            }
        }
    }))
}
