//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, Response, StatusCode, header},
    middleware::Next,
};
use log::*;
use tokio::sync::Mutex;

const LOG_TARGET: &str = "tari::ootle::indexer::rest_api::rate_limit";

#[derive(Debug, Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<RateLimitState>>,
    requests_per_window: u64,
    window_duration: Duration,
}

#[derive(Debug)]
struct RateLimitState {
    buckets: HashMap<IpAddr, TokenBucket>,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: u64,
    last_update: Instant,
}

impl RateLimiter {
    pub fn new(requests_per_window: u64, window_duration: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimitState {
                buckets: HashMap::new(),
            })),
            requests_per_window,
            window_duration,
        }
    }

    pub async fn middleware(
        &self,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response<Body>, StatusCode> {
        let ip = request
            .extensions()
            .get::<ConnectInfo<std::net::SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .unwrap_or_else(|| {
                // Fallback to loopback if no connection info (e.g. tests)
                "127.0.0.1".parse().unwrap()
            });

        let mut state = self.state.lock().await;
        let now = Instant::now();

        let bucket = state.buckets.entry(ip).or_insert_with(|| TokenBucket {
            tokens: self.requests_per_window,
            last_update: now,
        });

        // Refill tokens
        let elapsed = now.duration_since(bucket.last_update);
        let tokens_to_add = (elapsed.as_secs_f64() / self.window_duration.as_secs_f64() * self.requests_per_window as f64) as u64;
        
        if tokens_to_add > 0 {
            bucket.tokens = (bucket.tokens + tokens_to_add).min(self.requests_per_window);
            bucket.last_update = now;
        }

        if bucket.tokens > 0 {
            bucket.tokens -= 1;
            drop(state);
            Ok(next.run(request).await)
        } else {
            warn!(target: LOG_TARGET, "Rate limit exceeded for IP: {}", ip);
            let retry_after = self.window_duration.as_secs().max(1);
            let response = Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header(header::RETRY_AFTER, retry_after.to_string())
                .body(Body::from("Rate limit exceeded"))
                .unwrap();
            Ok(response)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConcurrentConnectionLimiter {
    state: Arc<Mutex<ConcurrentConnectionState>>,
    max_connections: usize,
}

#[derive(Debug)]
struct ConcurrentConnectionState {
    connections: HashMap<IpAddr, usize>,
}

impl ConcurrentConnectionLimiter {
    pub fn new(max_connections: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(ConcurrentConnectionState {
                connections: HashMap::new(),
            })),
            max_connections,
        }
    }

    pub async fn middleware(
        &self,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response<Body>, StatusCode> {
        let ip = request
            .extensions()
            .get::<ConnectInfo<std::net::SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .unwrap_or_else(|| "127.0.0.1".parse().unwrap());

        {
            let mut state = self.state.lock().await;
            let count = state.connections.entry(ip).or_insert(0);
            if *count >= self.max_connections {
                warn!(target: LOG_TARGET, "Concurrent connection limit exceeded for IP: {}", ip);
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            *count += 1;
        }

        let response = next.run(request).await;

        {
            let mut state = self.state.lock().await;
            if let Some(count) = state.connections.get_mut(&ip) {
                if *count > 0 {
                    *count -= 1;
                }
            }
        }

        Ok(response)
    }
}
