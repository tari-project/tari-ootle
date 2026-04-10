use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct RateLimiter {
    limits: Arc<Mutex<std::collections::HashMap<String, (usize, usize)>>>, // IP: (request_count, time)
}

impl RateLimiter {
    // Middleware to handle rate limiting
    pub async fn handle_rate_limiting(req: Request<Body>, next: Next<Body>) -> Result<Response<Body>, (StatusCode, String)> {
        let ip = req.headers().get("X-Real-IP")
            .or_else(|| req.headers().get("X-Forwarded-For"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");

        let mut rate_limiter = RATE_LIMITER.lock().await;
        let limit = rate_limiter.limits.entry(ip.to_string()).or_default();

        if limit.0 >= limit.1 {
            // Rate limit exceeded
            return Err((StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded. Try again later.".to_string()));
        }

        limit.0 += 1; // Increment current count

        // Create a future to process the request
        let response = next.run(req).await;
        Ok(response)
    }
}