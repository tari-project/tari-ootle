//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Per-IP token-bucket rate limiting and SSE concurrent connection limiting
//! for the indexer REST API.
//!
//! ## Design
//!
//! Each `IpRateLimiter` holds a `DashMap<IpAddr, BucketState>` — one entry per
//! observed client IP. On every request the entry is updated in-place: elapsed
//! time since the last check refills tokens at the configured rate, then one
//! token is consumed. If no token is available the middleware returns
//! `429 Too Many Requests` with a `Retry-After` header (integer seconds).
//!
//! `SseConnectionLimiter` tracks concurrent SSE connections per IP using a
//! `DashMap<IpAddr, Arc<tokio::sync::Semaphore>>`.  The middleware acquires an
//! owned permit before running the handler; the permit is moved into a
//! `GuardedStream` that wraps the response body, so it is not released until
//! the SSE stream is actually closed by the client or server.

use std::{
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use futures::Stream;
use log::debug;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::rate_limit";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the current time as milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Extracts the client IP from the request.
///
/// If `trust_proxy` is true and an `X-Forwarded-For` header is present, the
/// first (leftmost) address in the header is used.  Otherwise the peer address
/// from the TCP connection (`ConnectInfo<SocketAddr>`) is used.
fn extract_ip(req: &Request, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(forwarded) = req.headers().get("x-forwarded-for") {
            if let Ok(s) = forwarded.to_str() {
                if let Some(first) = s.split(',').next() {
                    if let Ok(ip) = first.trim().parse::<IpAddr>() {
                        return ip;
                    }
                }
            }
        }
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))
}

fn too_many_requests(retry_after_secs: u64) -> Response {
    let mut res = Response::new(Body::from(
        r#"{"error":"Too Many Requests","message":"Rate limit exceeded, see Retry-After header"}"#,
    ));
    *res.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    let secs_str = retry_after_secs.to_string();
    if let Ok(v) = HeaderValue::from_str(&secs_str) {
        res.headers_mut().insert(header::RETRY_AFTER, v);
    }
    res.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    res
}

// ---------------------------------------------------------------------------
// IpRateLimiter — per-IP token-bucket
// ---------------------------------------------------------------------------

struct BucketState {
    /// Tokens currently available (can be fractional during refill).
    tokens: f64,
    /// Timestamp of the last refill/check in milliseconds since Unix epoch.
    last_ms: u64,
}

/// Per-IP token-bucket rate limiter.
///
/// Create one instance per endpoint group (not shared between groups) to
/// prevent "starvation" where high load on one endpoint exhausts tokens for
/// another that happens to share the same limiter.
pub struct IpRateLimiter {
    buckets: DashMap<IpAddr, BucketState>,
    /// Burst capacity — also the initial token count for a new IP.
    capacity: f64,
    /// Tokens replenished per millisecond.
    tokens_per_ms: f64,
    /// Whether to trust `X-Forwarded-For` headers for IP extraction.
    trust_proxy: bool,
}

impl IpRateLimiter {
    /// `max_per_min` requests per minute, burst up to `max_per_min` tokens.
    pub fn per_min(max_per_min: u64, trust_proxy: bool) -> Arc<Self> {
        let cap = max_per_min as f64;
        Arc::new(Self {
            buckets: DashMap::new(),
            capacity: cap,
            tokens_per_ms: cap / 60_000.0,
            trust_proxy,
        })
    }

    /// `max_per_window` requests per `window`.
    pub fn per_window(max_per_window: u64, window: Duration, trust_proxy: bool) -> Arc<Self> {
        let cap = max_per_window as f64;
        Arc::new(Self {
            buckets: DashMap::new(),
            capacity: cap,
            tokens_per_ms: cap / window.as_millis() as f64,
            trust_proxy,
        })
    }

    /// Check and consume one token for `ip`.
    ///
    /// Returns `Ok(())` when a token is available or when the limiter is
    /// disabled (capacity == 0), or `Err(retry_after)` when the bucket is
    /// empty.
    pub fn check(&self, ip: IpAddr) -> Result<(), Duration> {
        // capacity == 0 means "no rate limit" for this endpoint group.
        if self.capacity == 0.0 {
            return Ok(());
        }
        let now = now_ms();
        let mut entry = self.buckets.entry(ip).or_insert_with(|| BucketState {
            tokens: self.capacity,
            last_ms: now,
        });

        let elapsed_ms = now.saturating_sub(entry.last_ms);
        let refill = elapsed_ms as f64 * self.tokens_per_ms;
        entry.tokens = (entry.tokens + refill).min(self.capacity);
        entry.last_ms = now;

        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            Ok(())
        } else {
            // How many ms until 1 token is available
            let ms_until = ((1.0 - entry.tokens) / self.tokens_per_ms).ceil() as u64;
            Err(Duration::from_millis(ms_until))
        }
    }
}

// ---------------------------------------------------------------------------
// Axum middleware function for IpRateLimiter
// ---------------------------------------------------------------------------

/// Axum middleware that applies `limiter` to every request.
///
/// Usage:
/// ```ignore
/// .layer(axum::middleware::from_fn_with_state(limiter, ip_rate_limit))
/// ```
pub async fn ip_rate_limit(
    axum::extract::State(limiter): axum::extract::State<Arc<IpRateLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    let ip = extract_ip(&req, limiter.trust_proxy);
    match limiter.check(ip) {
        Ok(()) => {
            debug!(target: LOG_TARGET, "Rate limit ok for {}", ip);
            next.run(req).await
        },
        Err(retry_after) => {
            let secs = retry_after.as_secs().max(1);
            debug!(target: LOG_TARGET, "Rate limit exceeded for {} — retry after {}s", ip, secs);
            too_many_requests(secs)
        },
    }
}

// ---------------------------------------------------------------------------
// SseConnectionLimiter — per-IP concurrent connection cap
// ---------------------------------------------------------------------------

/// Per-IP concurrent SSE connection limiter.
///
/// Uses a `DashMap<IpAddr, Arc<Semaphore>>` so each IP has its own semaphore
/// with `max_per_ip` permits.  The middleware acquires an `OwnedSemaphorePermit`
/// before running the SSE handler and moves it into a `GuardedStream` wrapper
/// around the response body, so the permit is held for the full duration of the
/// SSE stream and released when the connection closes.
pub struct SseConnectionLimiter {
    semaphores: DashMap<IpAddr, Arc<Semaphore>>,
    max_per_ip: usize,
    trust_proxy: bool,
}

impl SseConnectionLimiter {
    pub fn new(max_per_ip: usize, trust_proxy: bool) -> Arc<Self> {
        Arc::new(Self {
            semaphores: DashMap::new(),
            max_per_ip,
            trust_proxy,
        })
    }

    /// Returns `None` when the per-IP limit is reached, `Some(permit)` when a
    /// slot is available.  If `max_per_ip == 0` the limiter is disabled and
    /// always returns a permit (uses a semaphore with `usize::MAX` permits).
    fn try_acquire(&self, ip: IpAddr) -> Option<OwnedSemaphorePermit> {
        let max = if self.max_per_ip == 0 {
            usize::MAX
        } else {
            self.max_per_ip
        };
        let sem = self
            .semaphores
            .entry(ip)
            .or_insert_with(|| Arc::new(Semaphore::new(max)))
            .clone();
        sem.try_acquire_owned().ok()
    }
}

// ---------------------------------------------------------------------------
// GuardedStream — holds a semaphore permit until the body stream ends
// ---------------------------------------------------------------------------

/// A `Stream` adapter that holds a `OwnedSemaphorePermit` until the inner
/// stream is exhausted or dropped.  This ensures an SSE connection slot is
/// only released when the connection actually closes.
struct GuardedStream<S> {
    inner: S,
    /// Permit is held here; dropped when `GuardedStream` is dropped.
    _permit: OwnedSemaphorePermit,
}

impl<S: Stream + Unpin> Stream for GuardedStream<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

// ---------------------------------------------------------------------------
// Axum middleware function for SseConnectionLimiter
// ---------------------------------------------------------------------------

pub async fn sse_connection_limit(
    axum::extract::State(limiter): axum::extract::State<Arc<SseConnectionLimiter>>,
    req: Request,
    next: Next,
) -> Response {
    let ip = extract_ip(&req, limiter.trust_proxy);
    match limiter.try_acquire(ip) {
        None => {
            debug!(target: LOG_TARGET, "SSE connection limit exceeded for {}", ip);
            too_many_requests(5)
        },
        Some(permit) => {
            let response = next.run(req).await;
            // Wrap the response body so the semaphore permit is held until
            // the stream is fully consumed (i.e. the SSE connection closes).
            let (parts, body) = response.into_parts();
            let data_stream = body.into_data_stream();
            // GuardedStream holds the permit; Body::from_stream drives it.
            // The permit is released when GuardedStream is dropped, which
            // happens when the response body is fully consumed or the
            // connection is aborted — i.e. when the SSE session ends.
            let guarded = GuardedStream {
                inner: data_stream,
                _permit: permit,
            };
            let new_body = Body::from_stream(guarded);
            Response::from_parts(parts, new_body)
        },
    }
}
