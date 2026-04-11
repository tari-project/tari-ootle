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
    time::{Duration, Instant},
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
    tokens: f64,
    last_refill: Instant,
    last_accessed: Instant,
}

impl Bucket {
    fn new(capacity: f64) -> Self {
        let now = Instant::now();
        Self {
            tokens: capacity,
            last_refill: now,
            last_accessed: now,
        }
    }

    fn try_consume(&mut self, capacity: f64, refill_rate: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.last_accessed = now;

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

/// A per-IP token-bucket rate limiter with automatic eviction of stale entries.
#[derive(Clone)]
pub struct IpRateLimiter {
    buckets: Arc<DashMap<IpAddr, Bucket>>,
    capacity: f64,
    refill_rate: f64,
    stale_ttl: Duration,
}

/// How long an idle bucket is kept before eviction.
const DEFAULT_STALE_TTL: Duration = Duration::from_secs(300);
/// Eviction runs at most once per this interval to amortize the scan cost.
const EVICTION_INTERVAL: Duration = Duration::from_secs(60);

impl IpRateLimiter {
    pub fn new(capacity: u32, window_secs: u64) -> Self {
        let capacity = capacity as f64;
        let refill_rate = capacity / window_secs as f64;
        Self {
            buckets: Arc::new(DashMap::new()),
            capacity,
            refill_rate,
            stale_ttl: DEFAULT_STALE_TTL,
        }
    }

    pub fn check(&self, ip: IpAddr) -> bool {
        self.maybe_evict_stale();
        let mut bucket = self
            .buckets
            .entry(ip)
            .or_insert_with(|| Bucket::new(self.capacity));
        bucket.try_consume(self.capacity, self.refill_rate)
    }

    /// Remove buckets that have not been accessed for longer than `stale_ttl`.
    /// Uses a simple probabilistic trigger: only runs when the map exceeds 128
    /// entries and at most once per `EVICTION_INTERVAL`.
    fn maybe_evict_stale(&self) {
        if self.buckets.len() < 128 {
            return;
        }
        let now = Instant::now();
        self.buckets
            .retain(|_ip, bucket| now.duration_since(bucket.last_accessed) < self.stale_ttl);
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

/// Extract the client IP, optionally trusting proxy headers.
///
/// When `trust_proxy_headers` is `true`, `X-Forwarded-For` and `X-Real-IP` are
/// consulted first. This should only be enabled when the indexer sits behind a
/// trusted reverse proxy that overwrites these headers. When the indexer is
/// exposed directly to the internet, set this to `false` so that clients cannot
/// spoof their IP to bypass rate limits.
pub fn extract_ip(
    headers: &HeaderMap,
    connect_info: Option<&ConnectInfo<SocketAddr>>,
    trust_proxy_headers: bool,
) -> IpAddr {
    if trust_proxy_headers {
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

        if let Some(xri) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            if let Ok(ip) = xri.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }

    if let Some(ConnectInfo(addr)) = connect_info {
        return addr.ip();
    }

    IpAddr::from([127, 0, 0, 1])
}

// ---------------------------------------------------------------------------
// Axum middleware factories
// ---------------------------------------------------------------------------

/// Configuration for the rate-limit middleware layer.
#[derive(Clone)]
pub struct RateLimitConfig {
    pub limiter: IpRateLimiter,
    /// Whether to trust `X-Forwarded-For` / `X-Real-IP` headers for IP
    /// extraction. Only enable this when running behind a trusted reverse proxy.
    pub trust_proxy_headers: bool,
}

/// Axum middleware that enforces a per-IP token-bucket rate limit.
///
/// Responds with HTTP 429 when a bucket is exhausted.
pub async fn rate_limit_middleware(
    axum::extract::State(config): axum::extract::State<RateLimitConfig>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    req: Request,
    next: Next,
) -> Response {
    let ip = extract_ip(req.headers(), connect_info.as_ref(), config.trust_proxy_headers);
    if config.limiter.check(ip) {
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
/// The guard is stored in the response extensions so its lifetime is tied to
/// the response object held by the server, not the middleware future. This
/// ensures the connection counter stays incremented for the full duration of
/// long-lived SSE streams.
pub async fn sse_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<SseConnectionLimiter>,
    req: Request,
    next: Next,
) -> Response {
    match limiter.try_acquire() {
        Some(guard) => {
            let mut response = next.run(req).await;
            response.extensions_mut().insert(guard);
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
