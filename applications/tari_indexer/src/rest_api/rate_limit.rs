//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Rate limiting middleware for the indexer REST API.
//!
//! This module implements per-IP rate limiting using a token bucket algorithm.
//! It supports configurable rate limits for different endpoints and returns
//! HTTP 429 with `Retry-After` header when limits are exceeded.

use std::{
    collections::HashMap,
    fmt,
    net::IpAddr,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tower::{Layer, Service};

/// Rate limit configuration for the indexer REST API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Rate limit for POST /transactions endpoint (requests per minute per IP)
    #[serde(default = "default_transactions_rate_limit")]
    pub transactions_per_minute: u32,
    
    /// Rate limit for POST /transactions/dry-run endpoint (requests per minute per IP)
    #[serde(default = "default_dry_run_rate_limit")]
    pub dry_run_per_minute: u32,
    
    /// Rate limit for POST /substates/fetch endpoint (requests per minute per IP)
    #[serde(default = "default_substates_fetch_rate_limit")]
    pub substates_fetch_per_minute: u32,
    
    /// Rate limit for POST /utxos/fetch endpoint (requests per minute per IP)
    #[serde(default = "default_utxos_fetch_rate_limit")]
    pub utxos_fetch_per_minute: u32,
    
    /// Rate limit for GET /non-fungibles endpoint (requests per minute per IP)
    #[serde(default = "default_non_fungibles_rate_limit")]
    pub non_fungibles_per_minute: u32,
    
    /// Maximum concurrent SSE connections per IP
    #[serde(default = "default_sse_max_connections")]
    pub sse_max_connections_per_ip: u32,
}

fn default_transactions_rate_limit() -> u32 { 20 }
fn default_dry_run_rate_limit() -> u32 { 10 }
fn default_substates_fetch_rate_limit() -> u32 { 30 }
fn default_utxos_fetch_rate_limit() -> u32 { 15 }
fn default_non_fungibles_rate_limit() -> u32 { 30 }
fn default_sse_max_connections() -> u32 { 3 }

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            transactions_per_minute: default_transactions_rate_limit(),
            dry_run_per_minute: default_dry_run_rate_limit(),
            substates_fetch_per_minute: default_substates_fetch_rate_limit(),
            utxos_fetch_per_minute: default_utxos_fetch_rate_limit(),
            non_fungibles_per_minute: default_non_fungibles_rate_limit(),
            sse_max_connections_per_ip: default_sse_max_connections(),
        }
    }
}

/// Token bucket for rate limiting.
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: u32, refill_rate: f64) -> Self {
        let max_tokens = max_tokens as f64;
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self, tokens: f64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        let new_tokens = elapsed * self.refill_rate;
        self.tokens = (self.tokens + new_tokens).min(self.max_tokens);
        self.last_refill = Instant::now();
    }

    fn time_until_next_token(&self) -> Duration {
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            let tokens_needed = 1.0 - self.tokens;
            Duration::from_secs_f64(tokens_needed / self.refill_rate)
        }
    }
}

/// Internal state for rate limiting.
struct RateLimiterState {
    buckets: HashMap<IpAddr, TokenBucket>,
    sse_connections: HashMap<IpAddr, u32>,
}

impl RateLimiterState {
    fn new() -> Self {
        Self {
            buckets: HashMap::new(),
            sse_connections: HashMap::new(),
        }
    }
}

/// Rate limiter that manages per-IP rate limits.
#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<RwLock<RateLimiterState>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(RateLimiterState::new())),
            config,
        }
    }

    /// Get the rate limit for a specific endpoint type.
    fn get_rate_limit(&self, endpoint: RateLimitEndpoint) -> u32 {
        match endpoint {
            RateLimitEndpoint::Transactions => self.config.transactions_per_minute,
            RateLimitEndpoint::DryRun => self.config.dry_run_per_minute,
            RateLimitEndpoint::SubstatesFetch => self.config.substates_fetch_per_minute,
            RateLimitEndpoint::UtxosFetch => self.config.utxos_fetch_per_minute,
            RateLimitEndpoint::NonFungibles => self.config.non_fungibles_per_minute,
        }
    }

    /// Check if a request should be rate limited.
    /// Returns None if allowed, or Some(RetryAfterValue) if rate limited.
    pub fn check_rate_limit(&self, ip: IpAddr, endpoint: RateLimitEndpoint) -> Option<RetryAfterValue> {
        let rate_limit = self.get_rate_limit(endpoint);
        let refill_rate = rate_limit as f64 / 60.0; // per second

        let mut state = self.state.write().ok()?;
        let bucket = state.buckets.entry(ip).or_insert_with(|| {
            TokenBucket::new(rate_limit, refill_rate)
        });

        // Update refill rate if config changed
        bucket.refill_rate = refill_rate;
        bucket.max_tokens = rate_limit as f64;

        if bucket.try_consume(1.0) {
            None
        } else {
            Some(RetryAfterValue {
                seconds: bucket.time_until_next_token().as_secs() as u32 + 1,
            })
        }
    }

    /// Check and increment SSE connection count.
    /// Returns true if allowed, false if max connections exceeded.
    pub fn check_sse_connection(&self, ip: IpAddr) -> bool {
        let mut state = match self.state.write() {
            Ok(state) => state,
            Err(_) => return false,
        };
        let count = state.sse_connections.entry(ip).or_insert(0);
        
        if *count < self.config.sse_max_connections_per_ip {
            *count += 1;
            true
        } else {
            false
        }
    }

    /// Decrement SSE connection count for an IP.
    pub fn release_sse_connection(&self, ip: IpAddr) {
        if let Ok(mut state) = self.state.write() {
            if let Some(count) = state.sse_connections.get_mut(&ip) {
                if *count > 0 {
                    *count -= 1;
                }
            }
        }
    }

    /// Get current SSE connection count for an IP.
    #[allow(dead_code)]
    pub fn get_sse_connection_count(&self, ip: IpAddr) -> u32 {
        let state = match self.state.read() {
            Ok(state) => state,
            Err(_) => return 0,
        };
        state.sse_connections.get(&ip).copied().unwrap_or(0)
    }
}

/// Endpoint types for rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitEndpoint {
    Transactions,
    DryRun,
    SubstatesFetch,
    UtxosFetch,
    NonFungibles,
}

/// Value to include in Retry-After header.
#[derive(Debug, Clone)]
pub struct RetryAfterValue {
    pub seconds: u32,
}

/// Error response for rate limit exceeded.
#[derive(Debug)]
pub struct RateLimitExceeded {
    pub retry_after: u32,
}

impl fmt::Display for RateLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rate limit exceeded. Retry after {} seconds.", self.retry_after)
    }
}

impl IntoResponse for RateLimitExceeded {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": format!("Rate limit exceeded. Retry after {} seconds.", self.retry_after)
        }));
        
        let mut response = body.into_response();
        let headers = response.headers_mut();
        headers.insert(
            "Retry-After",
            HeaderValue::from(self.retry_after),
        );
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        response
    }
}

/// Extract client IP from request.
pub fn extract_client_ip(request: &Request) -> IpAddr {
    // Try to get IP from various sources (in order of preference)
    
    // 1. X-Forwarded-For header (if behind a proxy)
    if let Some(header) = request.headers().get("X-Forwarded-For") {
        if let Ok(value) = header.to_str() {
            // X-Forwarded-For can contain multiple IPs, take the first one
            if let Some(ip) = value.split(',').next() {
                if let Ok(ip) = ip.trim().parse() {
                    return ip;
                }
            }
        }
    }

    // 2. X-Real-IP header
    if let Some(header) = request.headers().get("X-Real-IP") {
        if let Ok(value) = header.to_str() {
            if let Ok(ip) = value.parse() {
                return ip;
            }
        }
    }

    // 3. ConnectInfo from axum (set by server)
    if let Some(conn_info) = request.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>() {
        return conn_info.0.ip();
    }

    // 4. Fallback to remote address from extensions
    if let Some(remote_addr) = request.extensions().get::<std::net::SocketAddr>() {
        return remote_addr.ip();
    }

    // 5. Default fallback (unlikely to be hit in practice)
    IpAddr::from([127, 0, 0, 1])
}

/// Rate limit middleware layer.
#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: RateLimiter,
    endpoint: RateLimitEndpoint,
}

impl RateLimitLayer {
    pub fn new(limiter: RateLimiter, endpoint: RateLimitEndpoint) -> Self {
        Self { limiter, endpoint }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
            endpoint: self.endpoint,
        }
    }
}

/// Rate limit middleware service.
#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: RateLimiter,
    endpoint: RateLimitEndpoint,
}

impl<S, B> Service<Request<B>> for RateLimitService<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn Send + Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&self, request: Request<B>) -> Self::Future {
        let limiter = self.limiter.clone();
        let endpoint = self.endpoint;
        let inner = self.inner.clone();

        Box::pin(async move {
            let ip = extract_client_ip(&request);
            
            if let Some(retry_after) = limiter.check_rate_limit(ip, endpoint) {
                return Ok(RateLimitExceeded {
                    retry_after: retry_after.seconds,
                }.into_response());
            }

            inner.call(request).await
        })
    }
}

/// SSE rate limit layer that tracks concurrent connections.
#[derive(Clone)]
pub struct SseRateLimitLayer {
    limiter: RateLimiter,
}

impl SseRateLimitLayer {
    pub fn new(limiter: RateLimiter) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for SseRateLimitLayer {
    type Service = SseRateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SseRateLimitService {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

/// SSE rate limit middleware service.
#[derive(Clone)]
pub struct SseRateLimitService<S> {
    inner: S,
    limiter: RateLimiter,
}

impl<S, B> Service<Request<B>> for SseRateLimitService<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn Send + Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&self, request: Request<B>) -> Self::Future {
        let limiter = self.limiter.clone();
        let inner = self.inner.clone();

        Box::pin(async move {
            let ip = extract_client_ip(&request);
            
            if !limiter.check_sse_connection(ip) {
                return Ok(RateLimitExceeded {
                    retry_after: 1,
                }.into_response());
            }

            inner.call(request).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10, 10.0 / 60.0); // 10 per minute = 10/60 per second
        
        // Should be able to consume up to 10 tokens
        for i in 0..10 {
            assert!(bucket.try_consume(1.0), "Should consume token {}", i);
        }
        
        // 11th should fail
        assert!(!bucket.try_consume(1.0), "Should not consume 11th token");
    }

    #[test]
    fn test_rate_limiter() {
        let config = RateLimitConfig {
            transactions_per_minute: 5,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let ip = IpAddr::from([127, 0, 0, 1]);

        // Should allow 5 requests
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(ip, RateLimitEndpoint::Transactions).is_none());
        }

        // 6th should be rate limited
        assert!(limiter.check_rate_limit(ip, RateLimitEndpoint::Transactions).is_some());
    }

    #[test]
    fn test_sse_connections() {
        let config = RateLimitConfig {
            sse_max_connections_per_ip: 2,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let ip = IpAddr::from([127, 0, 0, 1]);

        assert!(limiter.check_sse_connection(ip));
        assert!(limiter.check_sse_connection(ip));
        assert!(!limiter.check_sse_connection(ip)); // 3rd should fail

        limiter.release_sse_connection(ip);
        assert!(limiter.check_sse_connection(ip)); // Should work after release
    }
}
