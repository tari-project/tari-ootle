//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Per-IP token-bucket rate limiting middleware for the indexer REST API.
//!
//! Each IP address gets its own token bucket. The `(capacity, window)` pair is
//! configured per endpoint group; the bucket refills continuously at
//! `capacity / window` tokens per second. A request consumes one token; when
//! the bucket is empty the request is rejected with HTTP 429 and a
//! `Retry-After` header set to the computed time until the next token is
//! available (clamped to a minimum of 1 second).
//!
//! SSE / streaming endpoints use a per-IP concurrent-connection counter
//! instead of a token bucket — the slot is held for the full lifetime of the
//! response body and released on disconnect.

use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
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
    /// Reference point for `last_eviction_secs` — captured once at construction.
    start: Instant,
    /// Seconds since `start` at which eviction last ran. Single AtomicU64 + CAS
    /// claims the eviction window, so only one thread scans per interval.
    last_eviction_secs: Arc<AtomicU64>,
}

/// How long an idle bucket is kept before eviction.
const DEFAULT_STALE_TTL: Duration = Duration::from_secs(300);
/// Eviction runs at most once per this interval to amortize the scan cost.
const EVICTION_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RefillRate {
    capacity: f64,
    window: Duration,
}

impl RefillRate {
    pub fn new(capacity: f64, window: Duration) -> Option<Self> {
        if window.as_secs() == 0 {
            None
        } else {
            Some(Self { capacity, window })
        }
    }

    pub fn calculate(&self) -> f64 {
        self.capacity / self.window.as_secs_f64()
    }
}

impl IpRateLimiter {
    pub fn new(refill_rate: RefillRate) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            capacity: refill_rate.capacity,
            refill_rate: refill_rate.calculate(),
            stale_ttl: DEFAULT_STALE_TTL,
            start: Instant::now(),
            last_eviction_secs: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn check(&self, ip: IpAddr) -> Result<(), Duration> {
        self.maybe_evict_stale();
        let mut bucket = self.buckets.entry(ip).or_insert_with(|| Bucket::new(self.capacity));
        bucket.try_consume(self.capacity, self.refill_rate)
    }

    /// Remove buckets that have not been accessed for longer than `stale_ttl`.
    /// Uses a simple probabilistic trigger: only runs when the map exceeds 128
    /// entries and at most once per `EVICTION_INTERVAL`. Concurrent callers
    /// race on a single CAS — only the winner runs the scan.
    fn maybe_evict_stale(&self) {
        if self.buckets.len() < 128 {
            return;
        }
        let now = Instant::now();
        let now_secs = now.duration_since(self.start).as_secs();
        let last = self.last_eviction_secs.load(Ordering::Relaxed);
        if now_secs.saturating_sub(last) < EVICTION_INTERVAL.as_secs() {
            return;
        }
        if self
            .last_eviction_secs
            .compare_exchange(last, now_secs, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
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
    connections: Arc<DashMap<IpAddr, AtomicUsize>>,
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
    ///
    /// The CAS runs while the DashMap entry guard is alive, so the shard
    /// write-lock is held across the increment. This makes the increment
    /// atomic with respect to the cleanup performed in `SseConnectionGuard`'s
    /// `Drop` — neither side can observe a half-applied state.
    pub fn try_acquire(&self, ip: IpAddr) -> Option<SseConnectionGuard> {
        let entry = self.connections.entry(ip).or_insert_with(|| AtomicUsize::new(0));
        let counter = entry.value();

        let mut current = counter.load(Ordering::Relaxed);
        loop {
            if current >= self.max_per_ip {
                return None;
            }
            match counter.compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => {
                    return Some(SseConnectionGuard {
                        ip,
                        connections: self.connections.clone(),
                    });
                },
                Err(actual) => current = actual,
            }
        }
    }
}

/// RAII guard that decrements the per-IP connection counter when dropped, and
/// removes the map entry when the count reaches zero.
pub struct SseConnectionGuard {
    ip: IpAddr,
    connections: Arc<DashMap<IpAddr, AtomicUsize>>,
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        // `remove_if` runs the closure under the shard write-lock. Because
        // `try_acquire` also holds the shard lock across its increment, there
        // is no window in which a concurrent acquire could see (and CAS on) a
        // counter that we are about to remove.
        self.connections
            .remove_if(&self.ip, |_, counter| counter.fetch_sub(1, Ordering::AcqRel) == 1);
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
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) &&
            let Some(first) = xff.split(',').next() &&
            let Ok(ip) = first.trim().parse::<IpAddr>()
        {
            return ip;
        }

        if let Some(xri) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) &&
            let Ok(ip) = xri.trim().parse::<IpAddr>()
        {
            return ip;
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
    pub enabled: bool,
    pub limiter: IpRateLimiter,
    /// Whether to trust `X-Forwarded-For` / `X-Real-IP` headers for IP
    /// extraction. Only enable this when running behind a trusted reverse proxy.
    pub trust_proxy_headers: bool,
}

/// Configuration for the SSE connection-limit middleware layer.
#[derive(Clone)]
pub struct SseLimitConfig {
    pub enabled: bool,
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
    if !config.enabled {
        return next.run(req).await;
    }
    let connect_info = req.extensions().get::<ConnectInfo<SocketAddr>>().copied();
    let ip = extract_ip(req.headers(), connect_info.as_ref(), config.trust_proxy_headers);
    match config.limiter.check(ip) {
        Ok(()) => next.run(req).await,
        Err(retry_after) => (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, retry_after.as_secs().to_string())],
            axum::Json(serde_json::json!({
                "error": format!("Rate limit exceeded. Please try again in {} seconds.", retry_after.as_secs())
            })),
        )
            .into_response(),
    }
}

/// Axum middleware that enforces a per-IP concurrent-connection limit on SSE
/// endpoints.
///
/// The guard is moved into the response **body**'s stream state, not the
/// response extensions. Axum/hyper drop response parts (including extensions)
/// once headers are flushed, so an extension-based guard would be released at
/// the start of the SSE stream rather than its end. By tying the guard to the
/// body stream, it lives until the stream is fully drained or the client
/// disconnects.
pub async fn sse_limit_middleware(
    axum::extract::State(config): axum::extract::State<SseLimitConfig>,
    req: Request,
    next: Next,
) -> Response {
    use futures::StreamExt;

    if !config.enabled {
        return next.run(req).await;
    }

    let connect_info = req.extensions().get::<ConnectInfo<SocketAddr>>().copied();
    let ip = extract_ip(req.headers(), connect_info.as_ref(), config.trust_proxy_headers);
    match config.limiter.try_acquire(ip) {
        Some(guard) => {
            let response = next.run(req).await;
            let (parts, body) = response.into_parts();
            let stream = body.into_data_stream();
            // `unfold` carries `guard` in its state. When the stream completes
            // or is dropped (client disconnect, server shutdown), the state is
            // dropped and the guard releases the connection slot.
            let stream = futures::stream::unfold((stream, guard), |(mut s, g)| async move {
                s.next().await.map(|item| (item, (s, g)))
            });
            let body = axum::body::Body::from_stream(stream);
            Response::from_parts(parts, body)
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

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, sync::atomic::AtomicUsize, thread};

    use axum::http::HeaderValue;

    use super::*;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::from(Ipv4Addr::new(a, b, c, d))
    }

    fn rate(capacity: f64, window_secs: u64) -> RefillRate {
        RefillRate::new(capacity, Duration::from_secs(window_secs)).unwrap()
    }

    // ---------------------------------------------------------------- Bucket --

    #[test]
    fn bucket_allows_capacity_then_rejects() {
        let mut b = Bucket::new(3.0);
        assert!(b.try_consume(3.0, 1.0).is_ok());
        assert!(b.try_consume(3.0, 1.0).is_ok());
        assert!(b.try_consume(3.0, 1.0).is_ok());
        assert!(b.try_consume(3.0, 1.0).is_err());
    }

    #[test]
    fn bucket_retry_after_is_at_least_one_second() {
        let mut b = Bucket::new(1.0);
        let _ = b.try_consume(1.0, 1.0);
        let err = b.try_consume(1.0, 1.0).unwrap_err();
        assert!(err >= Duration::from_secs(1));
    }

    #[test]
    fn bucket_refills_proportionally_to_elapsed_time() {
        // capacity 2, refill 1 token/sec
        let mut b = Bucket::new(2.0);
        assert!(b.try_consume(2.0, 1.0).is_ok());
        assert!(b.try_consume(2.0, 1.0).is_ok());
        assert!(b.try_consume(2.0, 1.0).is_err());

        // Backdate to simulate 2s elapsed → 2 tokens refilled.
        b.last_refill = Instant::now() - Duration::from_secs(2);
        assert!(b.try_consume(2.0, 1.0).is_ok());
        assert!(b.try_consume(2.0, 1.0).is_ok());
        assert!(b.try_consume(2.0, 1.0).is_err());
    }

    #[test]
    fn bucket_caps_at_capacity_after_long_idle() {
        let mut b = Bucket::new(2.0);
        assert!(b.try_consume(2.0, 1.0).is_ok());
        assert!(b.try_consume(2.0, 1.0).is_ok());
        // Pretend an hour has elapsed; refill must still cap at 2.
        b.last_refill = Instant::now() - Duration::from_secs(3600);
        assert!(b.try_consume(2.0, 1.0).is_ok());
        assert!(b.try_consume(2.0, 1.0).is_ok());
        assert!(b.try_consume(2.0, 1.0).is_err());
    }

    // -------------------------------------------------------- IpRateLimiter --

    #[test]
    fn ip_rate_limiter_isolates_per_ip() {
        let limiter = IpRateLimiter::new(rate(1.0, 60));
        let a = ip(10, 0, 0, 1);
        let b = ip(10, 0, 0, 2);
        assert!(limiter.check(a).is_ok());
        assert!(limiter.check(a).is_err());
        // b has its own bucket.
        assert!(limiter.check(b).is_ok());
        assert!(limiter.check(b).is_err());
    }

    #[test]
    fn ip_rate_limiter_concurrent_check_respects_capacity() {
        let limiter = IpRateLimiter::new(rate(10.0, 60));
        let target = ip(10, 0, 0, 1);
        let success = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..32)
            .map(|_| {
                let limiter = limiter.clone();
                let success = success.clone();
                thread::spawn(move || {
                    if limiter.check(target).is_ok() {
                        success.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        // Refill over a few-ms test run is well below 1 token, so exactly capacity must succeed.
        assert_eq!(success.load(Ordering::SeqCst), 10);
    }

    // --------------------------------------------------- SseConnectionLimiter --

    #[test]
    fn sse_acquires_up_to_max_then_rejects() {
        let limiter = SseConnectionLimiter::new(3);
        let target = ip(10, 0, 0, 1);
        let g1 = limiter.try_acquire(target).unwrap();
        let g2 = limiter.try_acquire(target).unwrap();
        let g3 = limiter.try_acquire(target).unwrap();
        assert!(limiter.try_acquire(target).is_none());
        drop(g1);
        let _g4 = limiter.try_acquire(target).unwrap();
        drop((g2, g3));
    }

    #[test]
    fn sse_drop_decrements_and_removes_when_last() {
        let limiter = SseConnectionLimiter::new(2);
        let target = ip(10, 0, 0, 1);
        let g1 = limiter.try_acquire(target).unwrap();
        let g2 = limiter.try_acquire(target).unwrap();
        assert_eq!(limiter.connections.len(), 1);
        drop(g1);
        // Counter now 1 — entry must remain.
        assert_eq!(limiter.connections.len(), 1);
        drop(g2);
        // Last guard dropped — entry removed.
        assert_eq!(limiter.connections.len(), 0);
    }

    #[test]
    fn sse_isolates_per_ip() {
        let limiter = SseConnectionLimiter::new(1);
        let a = ip(10, 0, 0, 1);
        let b = ip(10, 0, 0, 2);
        let _ga = limiter.try_acquire(a).unwrap();
        assert!(limiter.try_acquire(a).is_none());
        // b has its own counter.
        let _gb = limiter.try_acquire(b).unwrap();
        assert!(limiter.try_acquire(b).is_none());
    }

    #[test]
    fn sse_concurrent_acquires_never_exceed_max() {
        let limiter = SseConnectionLimiter::new(3);
        let target = ip(10, 0, 0, 1);
        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..32)
            .map(|_| {
                let limiter = limiter.clone();
                let active = active.clone();
                let max_seen = max_seen.clone();
                thread::spawn(move || {
                    if let Some(_g) = limiter.try_acquire(target) {
                        let cur = active.fetch_add(1, Ordering::SeqCst) + 1;
                        max_seen.fetch_max(cur, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(20));
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert!(max_seen.load(Ordering::SeqCst) <= 3);
        // After every guard has dropped, the entry must have been cleaned up.
        assert_eq!(limiter.connections.len(), 0);
    }

    // -------------------------------------------------------------- extract_ip --

    fn ci(addr: &str) -> ConnectInfo<SocketAddr> {
        ConnectInfo(addr.parse().unwrap())
    }

    #[test]
    fn extract_ip_falls_back_to_localhost_when_no_info() {
        let headers = HeaderMap::new();
        assert_eq!(extract_ip(&headers, None, false), IpAddr::from([127, 0, 0, 1]));
        assert_eq!(extract_ip(&headers, None, true), IpAddr::from([127, 0, 0, 1]));
    }

    #[test]
    fn extract_ip_uses_connect_info() {
        let conn = ci("203.0.113.7:55512");
        assert_eq!(
            extract_ip(&HeaderMap::new(), Some(&conn), false),
            IpAddr::from([203, 0, 113, 7])
        );
    }

    #[test]
    fn extract_ip_ignores_proxy_headers_when_untrusted() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4"));
        headers.insert("x-real-ip", HeaderValue::from_static("9.9.9.9"));
        let conn = ci("203.0.113.7:55512");
        // trust=false → proxy headers ignored, ConnectInfo wins.
        assert_eq!(extract_ip(&headers, Some(&conn), false), IpAddr::from([203, 0, 113, 7]));
    }

    #[test]
    fn extract_ip_takes_xff_first_entry_when_trusted() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4, 5.6.7.8"));
        let conn = ci("203.0.113.7:55512");
        assert_eq!(extract_ip(&headers, Some(&conn), true), IpAddr::from([1, 2, 3, 4]));
    }

    #[test]
    fn extract_ip_falls_back_to_xri_when_xff_missing_and_trusted() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("9.9.9.9"));
        assert_eq!(extract_ip(&headers, None, true), IpAddr::from([9, 9, 9, 9]));
    }

    #[test]
    fn extract_ip_falls_through_invalid_proxy_headers_to_connect_info() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("not-an-ip"));
        headers.insert("x-real-ip", HeaderValue::from_static("also-garbage"));
        let conn = ci("203.0.113.7:55512");
        assert_eq!(extract_ip(&headers, Some(&conn), true), IpAddr::from([203, 0, 113, 7]));
    }

    #[test]
    fn refill_rate_rejects_zero_and_subsecond_windows() {
        assert!(RefillRate::new(10.0, Duration::ZERO).is_none());
        assert!(RefillRate::new(10.0, Duration::from_millis(500)).is_none());
        assert!(RefillRate::new(10.0, Duration::from_secs(1)).is_some());
    }
}
