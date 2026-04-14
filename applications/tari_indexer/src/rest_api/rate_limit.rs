//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Per-IP token-bucket rate limiting middleware for the indexer REST API.
//!
//! Each IP address gets its own token bucket. Tokens refill continuously at a
//! rate of `capacity / window_secs` tokens per second. A request consumes one
//! token; when the bucket is empty the request is rejected with HTTP 429 with
//! a `Retry-After` header indicating the window duration.
//!
//! SSE / streaming endpoints use a per-IP concurrent-connection counter
//! instead of a token bucket.

use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request},
    http::{HeaderMap, StatusCode, header},
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

    /// Try to consume one token. Returns `Ok(())` on success, or
    /// `Err(duration)` with the time until the next token is available.
    fn try_consume(&mut self, capacity: f64, refill_rate: f64) -> Result<(), Duration> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.last_accessed = now;

        self.tokens = (self.tokens + elapsed * refill_rate).min(capacity);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            let tokens_needed = 1.0 - self.tokens;
            let seconds = if refill_rate > 0.0 {
                (tokens_needed / refill_rate).max(1.0)
            } else {
                60.0
            };
            Err(Duration::from_secs_f64(seconds))
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
    last_eviction: Arc<Mutex<Instant>>,
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
            last_eviction: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub fn check(&self, ip: IpAddr) -> Result<(), Duration> {
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
        // Gate eviction by EVICTION_INTERVAL to amortize the scan cost.
        let mut last = self.last_eviction.lock().unwrap();
        if now.duration_since(*last) < EVICTION_INTERVAL {
            return;
        }
        *last = now;
        drop(last);
        self.buckets
            .retain(|_ip, bucket| now.duration_since(bucket.last_accessed) < self.stale_ttl);
    }
}

// ---------------------------------------------------------------------------
// SSE per-IP concurrent-connection limiter
// ---------------------------------------------------------------------------

/// Limits the number of concurrent SSE connections per IP address.
#[derive(Clone)]
pub struct SseConnectionLimiter {
    connections: Arc<DashMap<IpAddr, Arc<AtomicUsize>>>,
    max_per_ip: usize,
}

impl SseConnectionLimiter {
    pub fn new(max_per_ip: usize) -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            max_per_ip,
        }
    }

    /// Attempt to acquire a connection slot for the given IP. Returns a guard
    /// that releases the slot on drop, or `None` if the per-IP limit has been
    /// reached.
    pub fn try_acquire(&self, ip: IpAddr) -> Option<SseConnectionGuard> {
        let counter = self
            .connections
            .entry(ip)
            .or_insert_with(|| Arc::new(AtomicUsize::new(0)))
            .clone();

        // Use a compare-exchange loop to avoid going over the per-IP limit.
        let mut current = counter.load(Ordering::Relaxed);
        loop {
            if current >= self.max_per_ip {
                return None;
            }
            match counter.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(SseConnectionGuard { counter });
                },
                Err(actual) => current = actual,
            }
        }
    }
}

/// RAII guard that decrements the per-IP connection counter when dropped.
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

/// Configuration for the SSE connection-limit middleware layer.
#[derive(Clone)]
pub struct SseLimitConfig {
    pub limiter: SseConnectionLimiter,
    /// Whether to trust `X-Forwarded-For` / `X-Real-IP` headers for IP
    /// extraction. Only enable this when running behind a trusted reverse proxy.
    pub trust_proxy_headers: bool,
}

/// Axum middleware that enforces a per-IP token-bucket rate limit.
///
/// Responds with HTTP 429 (with `Retry-After` header) when a bucket is
/// exhausted.
///
/// `ConnectInfo` is extracted from request extensions rather than as a function
/// parameter to avoid type-inference issues with `route_layer` in axum 0.8.
pub async fn rate_limit_middleware(
    axum::extract::State(config): axum::extract::State<RateLimitConfig>,
    req: Request,
    next: Next,
) -> Response {
    let connect_info = req.extensions().get::<ConnectInfo<SocketAddr>>().cloned();
    let ip = extract_ip(req.headers(), connect_info.as_ref(), config.trust_proxy_headers);
    match config.limiter.check(ip) {
        Ok(()) => next.run(req).await,
        Err(retry_after) => {
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(
                    header::RETRY_AFTER,
                    retry_after.as_secs().to_string(),
                )],
                axum::Json(serde_json::json!({
                    "error": format!("Rate limit exceeded. Please try again in {} seconds.", retry_after.as_secs())
                })),
            )
                .into_response()
        },
    }
}

/// Axum middleware that enforces a per-IP concurrent-connection limit on SSE
/// endpoints.
///
/// The guard is stored in the response extensions so its lifetime is tied to
/// the response object held by the server, not the middleware future. This
/// ensures the connection counter stays incremented for the full duration of
/// long-lived SSE streams.
pub async fn sse_limit_middleware(
    axum::extract::State(config): axum::extract::State<SseLimitConfig>,
    req: Request,
    next: Next,
) -> Response {
    let connect_info = req.extensions().get::<ConnectInfo<SocketAddr>>().cloned();
    let ip = extract_ip(req.headers(), connect_info.as_ref(), config.trust_proxy_headers);
    match config.limiter.try_acquire(ip) {
        Some(guard) => {
            let mut response = next.run(req).await;
            // Wrap in Arc so we satisfy the Clone + Send + Sync + 'static bounds
            // required by http::Extensions::insert without deriving Clone on the
            // guard (which would cause a double-decrement bug).
            response.extensions_mut().insert(std::sync::Arc::new(guard));
            response
        },
        None => (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "60".to_string())],
            axum::Json(serde_json::json!({
                "error": "Too many concurrent SSE connections. Please try again later."
            })),
        )
            .into_response(),
    }
}
