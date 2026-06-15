use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::debug;

pub async fn track_metrics(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path   = request.uri().path().to_string();
    let start  = Instant::now();

    let response = next.run(request).await;

    let elapsed = start.elapsed();
    let status  = response.status().as_u16();

    debug!(method = %method, path = %path, status, latency_ms = elapsed.as_millis(), "HTTP request");

    response
}
