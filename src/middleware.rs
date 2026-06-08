// Copyright 2024-2026 Veo Technologies
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::metrics::{HTTP_REQUEST_DURATION_SECONDS, HTTP_REQUESTS_PENDING, HTTP_REQUESTS_TOTAL, HTTP_RESPONSE_BODY_SIZE, excluded_path};
use axum::{
    body::HttpBody,
    extract::{MatchedPath, Request},
    response::Response,
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};
use tower::{Layer, Service};

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

        let clone = self.service.clone();
        let mut service = std::mem::replace(&mut self.service, clone);
        Box::pin(async move {
            let result = service.call(req).await;

            if !skip {
                HTTP_REQUESTS_PENDING.with_label_values(&[&method, &path]).dec();
            }

            let response = result?;
            let status = response.status().as_u16().to_string();

            if !skip {
                HTTP_REQUESTS_TOTAL.with_label_values(&[&method, &path, &status]).inc();

                let elapsed = start.elapsed().as_secs_f64();
                HTTP_REQUEST_DURATION_SECONDS
                    .with_label_values(&[&method, &path, &status])
                    .observe(elapsed);

                let size = response.body().size_hint().lower();
                HTTP_RESPONSE_BODY_SIZE.with_label_values(&[&method, &path]).observe(size as f64);
            }

            Ok(response)
        })
    }
}
