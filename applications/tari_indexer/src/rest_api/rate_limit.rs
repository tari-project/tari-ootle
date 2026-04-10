//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Token bucket rate limiter for per-IP rate limiting
#[derive(Debug, Clone)]
pub struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill_rate: f64) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    pub fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;
    }

    pub fn time_until_next_token(&self) -> Duration {
        if self.tokens >= 1.0 {
            Duration::from_secs(0)
        } else if self.refill_rate > 0.0 {
            let tokens_needed = 1.0 - self.tokens;
            let seconds = tokens_needed / self.refill_rate;
            Duration::from_secs_f64(seconds.max(1.0))
        } else {
            // If refill_rate is 0, return a maximum fallback duration
            Duration::from_secs(60)
        }
    }
}

/// Per-IP rate limiter using token bucket algorithm
#[derive(Clone)]
pub struct IpRateLimiter {
    buckets: Arc<Mutex<HashMap<IpAddr, TokenBucket>>>,
    capacity: u32,
    refill_rate: f64,
}

impl IpRateLimiter {
    pub fn new(requests_per_minute: u32) -> Self {
        let capacity = requests_per_minute;
        let refill_rate = requests_per_minute as f64 / 60.0; // tokens per second
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            capacity,
            refill_rate,
        }
    }

    pub fn check_rate_limit(&self, ip: IpAddr) -> Result<(), Duration> {
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(ip)
            .or_insert_with(|| TokenBucket::new(self.capacity, self.refill_rate));

        if bucket.try_consume() {
            Ok(())
        } else {
            Err(bucket.time_until_next_token())
        }
    }

    /// Cleanup old entries periodically to prevent memory leaks
    pub fn cleanup_old_entries(&self) {
        let mut buckets = self.buckets.lock().unwrap();
        buckets.retain(|_, bucket| {
            let elapsed = Instant::now().duration_since(bucket.last_refill);
            elapsed < Duration::from_secs(300) // Keep entries active in last 5 minutes
        });
    }
}

/// Middleware for per-IP rate limiting
pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let limiter = request
        .extensions()
        .get::<IpRateLimiter>()
        .expect("IpRateLimiter not found in request extensions");

    match limiter.check_rate_limit(addr.ip()) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            let mut response = Response::new(Body::from("Too Many Requests"));
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&retry_after.as_secs().to_string()).unwrap(),
            );
            response
        },
    }
}

/// Connection counter for SSE streams
#[derive(Clone)]
pub struct SseConnectionLimiter {
    connections: Arc<Mutex<HashMap<IpAddr, usize>>>,
    max_connections_per_ip: usize,
}

impl SseConnectionLimiter {
    pub fn new(max_connections_per_ip: usize) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            max_connections_per_ip,
        }
    }

    pub fn try_acquire(&self, ip: IpAddr) -> Result<SseConnectionGuard, ()> {
        let mut connections = self.connections.lock().unwrap();
        let count = connections.entry(ip).or_insert(0);

        if *count >= self.max_connections_per_ip {
            return Err(());
        }

        *count += 1;
        Ok(SseConnectionGuard {
            limiter: self.clone(),
            ip,
        })
    }

    fn release(&self, ip: IpAddr) {
        let mut connections = self.connections.lock().unwrap();
        if let Some(count) = connections.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                connections.remove(&ip);
            }
        }
    }
}

/// RAII guard for SSE connections
pub struct SseConnectionGuard {
    limiter: SseConnectionLimiter,
    ip: IpAddr,
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        self.limiter.release(self.ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_basic() {
        let mut bucket = TokenBucket::new(10, 1.0);
        assert!(bucket.try_consume());
        assert_eq!(bucket.tokens as u32, 9);
    }

    #[test]
    fn test_token_bucket_exhaustion() {
        let mut bucket = TokenBucket::new(2, 1.0);
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(!bucket.try_consume()); // Should fail
    }

    #[test]
    fn test_ip_rate_limiter() {
        let limiter = IpRateLimiter::new(60); // 60 req/min
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Should succeed
        assert!(limiter.check_rate_limit(ip).is_ok());
    }

    #[test]
    fn test_sse_connection_limiter() {
        let limiter = SseConnectionLimiter::new(3);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        let _guard1 = limiter.try_acquire(ip).unwrap();
        let _guard2 = limiter.try_acquire(ip).unwrap();
        let _guard3 = limiter.try_acquire(ip).unwrap();

        // Fourth connection should fail
        assert!(limiter.try_acquire(ip).is_err());

        // After dropping a guard, should succeed again
        drop(_guard1);
        assert!(limiter.try_acquire(ip).is_ok());
    }
}
