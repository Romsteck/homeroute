use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::{error, info, warn};

pub async fn request_logging(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // Skip health check endpoints
    if path == "/api/health" || path == "/health" {
        return next.run(request).await;
    }

    let start = Instant::now();
    let response = next.run(request).await;
    let duration = start.elapsed();
    let status = response.status().as_u16();

    match status {
        500..=599 => error!(
            http.method = %method,
            http.path = %path,
            http.status = status,
            http.duration_ms = duration.as_millis() as u64,
            "HTTP request failed"
        ),
        400..=499 => warn!(
            http.method = %method,
            http.path = %path,
            http.status = status,
            http.duration_ms = duration.as_millis() as u64,
            "HTTP client error"
        ),
        _ => info!(
            http.method = %method,
            http.path = %path,
            http.status = status,
            http.duration_ms = duration.as_millis() as u64,
            "HTTP request"
        ),
    }

    response
}
