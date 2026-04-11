//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Per-IP token-bucket rate limiting middleware for the indexer REST API.
//!
//! Each IP address gets its own token bucket. Tokens refill continuously at a
//! rate of `capacity / window_secs` tokens per second. A request consumes one
//! token; when the bucket is empty the request is rejected with HTTP 429.
//!
//! SSE / streaming endpoints use a simple per-IP concurrent-connection counter
//! instead of a token bucket.

use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use axum::{
    extract::{ConnectInfo, Request},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;

// ---------------------------------------------------------------------------
// Token-bucket state per IP
// ---------------------------------------------------------------------------

struct Bucket {
    /// Number of tokens currently available (fractional).
    tokens: f64,
    /// Wall-clock time of the last refill calculation.
    last_refill: Instant,
}

impl Bucket {
    fn new(capacity: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
        }
    }

    /// Attempt to consume one token. Returns `true` if the request is allowed.
    fn try_consume(&mut self, capacity: f64, refill_rate: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        // Refill tokens proportional to elapsed time, capped at capacity.
        self.tokens = (self.tokens + elapsed * refill_rate).min(capacity);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Rate limiter shared state
// ---------------------------------------------------------------------------

/// A per-IP token-bucket rate limiter.
///
/// `capacity`    – maximum burst size (tokens).
/// `window_secs` – time window for `capacity` requests (seconds).
///                 The steady-state refill rate is `capacity / window_secs` t/s.
#[derive(Clone)]
pub struct IpRateLimiter {
    buckets: Arc<DashMap<IpAddr, Bucket>>,
    capacity: f64,
    refill_rate: f64,
}

impl IpRateLimiter {
    /// Create a new limiter allowing `capacity` requests per `window_secs`.
    pub fn new(capacity: u32, window_secs: u64) -> Self {
        let capacity = capacity as f64;
        let refill_rate = capacity / window_secs as f64;
        Self {
            buckets: Arc::new(DashMap::new()),
            capacity,
            refill_rate,
        }
    }

    /// Returns `true` if the request from `ip` should be allowed.
    pub fn check(&self, ip: IpAddr) -> bool {
        let mut bucket = self
            .buckets
            .entry(ip)
            .or_insert_with(|| Bucket::new(self.capacity));
        bucket.try_consume(self.capacity, self.refill_rate)
    }
}

// ---------------------------------------------------------------------------
// SSE concurrent-connection limiter
// ---------------------------------------------------------------------------

/// Limits the number of concurrent SSE connections across all IPs.
#[derive(Clone)]
pub struct SseConnectionLimiter {
    current: Arc<AtomicUsize>,
    max: usize,
}

impl SseConnectionLimiter {
    pub fn new(max: usize) -> Self {
        Self {
            current: Arc::new(AtomicUsize::new(0)),
            max,
        }
    }

    /// Attempt to acquire a connection slot. Returns a guard that releases the
    /// slot on drop, or `None` if the limit has been reached.
    pub fn try_acquire(&self) -> Option<SseConnectionGuard> {
        // Use a compare-exchange loop to avoid going over the limit.
        let mut current = self.current.load(Ordering::Relaxed);
        loop {
            if current >= self.max {
                return None;
            }
            match self.current.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(SseConnectionGuard {
                        counter: self.current.clone(),
                    })
                },
                Err(actual) => current = actual,
            }
        }
    }
}

/// RAII guard that decrements the connection counter when dropped.
pub struct SseConnectionGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

// ---------------------------------------------------------------------------
// Helper: extract peer IP from request
// ---------------------------------------------------------------------------

/// Extract the client IP from `X-Forwarded-For`, `X-Real-IP`, or the TCP
/// peer address (populated by `axum::serve(...).into_make_service_with_connect_info`).
pub fn extract_ip(headers: &HeaderMap, connect_info: Option<&ConnectInfo<SocketAddr>>) -> IpAddr {
    // Prefer X-Forwarded-For (first entry) when behind a trusted proxy.
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            if let Ok(ip) = first.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }

    // Fall back to X-Real-IP.
    if let Some(xri) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        if let Ok(ip) = xri.trim().parse::<IpAddr>() {
            return ip;
        }
    }

    // Finally use the TCP peer address.
    if let Some(ConnectInfo(addr)) = connect_info {
        return addr.ip();
    }

    // Last resort: treat as loopback so the request is not silently dropped.
    IpAddr::from([127, 0, 0, 1])
}

// ---------------------------------------------------------------------------
// Axum middleware factories
// ---------------------------------------------------------------------------

/// Axum middleware that enforces a per-IP token-bucket rate limit.
///
/// Responds with HTTP 429 when a bucket is exhausted.
pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<IpRateLimiter>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    req: Request,
    next: Next,
) -> Response {
    let ip = extract_ip(req.headers(), connect_info.as_ref());
    if limiter.check(ip) {
        next.run(req).await
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "error": "Rate limit exceeded. Please slow down."
            })),
        )
            .into_response()
    }
}

/// Axum middleware that enforces a concurrent-connection limit on SSE endpoints.
///
/// Responds with HTTP 503 when the connection limit is reached.
pub async fn sse_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<SseConnectionLimiter>,
    req: Request,
    next: Next,
) -> Response {
    match limiter.try_acquire() {
        Some(_guard) => {
            // _guard is intentionally held for the lifetime of the request.
            // The future returned by `next.run(req)` keeps it alive until the
            // SSE stream is closed.
            let response = next.run(req).await;
            // _guard drops here, releasing the slot.
            drop(_guard);
            response
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "error": "Too many concurrent SSE connections. Please try again later."
            })),
        )
            .into_response(),
    }
}
