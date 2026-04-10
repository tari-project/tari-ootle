//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use mini_moka::sync::Cache;
use serde_json::json;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub struct RateLimitManager {
    limits: Arc<RateLimitCaches>,
    config: RateLimitConfig,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct SsePermit(Arc<Mutex<Option<OwnedSemaphorePermit>>>);

struct RateLimitCaches {
    transactions: Cache<IpAddr, Arc<Mutex<Bucket>>>,
    substates_fetch: Cache<IpAddr, Arc<Mutex<Bucket>>>,
    utxos_fetch: Cache<IpAddr, Arc<Mutex<Bucket>>>,
    non_fungibles: Cache<IpAddr, Arc<Mutex<Bucket>>>,
    dry_run: Cache<IpAddr, Arc<Mutex<Bucket>>>,
    sse_concurrency: Cache<IpAddr, Arc<Semaphore>>,
}

#[derive(Clone, Copy)]
pub struct RateLimitConfig {
    pub transactions_per_min: u64,
    pub substates_fetch_per_min: u64,
    pub utxos_fetch_per_min: u64,
    pub non_fungibles_per_min: u64,
    pub dry_run_per_min: u64,
    pub sse_max_concurrent: u32,
}

struct Bucket {
    count: u64,
    start_time: Instant,
}

impl RateLimitManager {
    pub fn new(config: crate::config::RateLimitConfig) -> Self {
        let cache_config = || {
            Cache::builder()
                .time_to_idle(Duration::from_secs(60))
                .build()
        };

        Self {
            limits: Arc::new(RateLimitCaches {
                transactions: cache_config(),
                substates_fetch: cache_config(),
                utxos_fetch: cache_config(),
                non_fungibles: cache_config(),
                dry_run: cache_config(),
                sse_concurrency: Cache::builder()
                    .time_to_idle(Duration::from_secs(3600))
                    .build(),
            }),
            config: RateLimitConfig {
                transactions_per_min: config.transactions_per_min,
                substates_fetch_per_min: config.substates_fetch_per_min,
                utxos_fetch_per_min: config.utxos_fetch_per_min,
                non_fungibles_per_min: config.non_fungibles_per_min,
                dry_run_per_min: config.dry_run_per_min,
                sse_max_concurrent: config.sse_max_concurrent,
            },
        }
    }

    async fn check_limit(
        &self,
        ip: IpAddr,
        cache: &Cache<IpAddr, Arc<Mutex<Bucket>>>,
        limit: u64,
    ) -> Result<(), Duration> {
        if limit == 0 {
            return Ok(());
        }

        let bucket_arc = if let Some(bucket) = cache.get(&ip) {
            bucket
        } else {
            let bucket = Arc::new(Mutex::new(Bucket {
                count: 0,
                start_time: Instant::now(),
            }));
            cache.insert(ip, bucket.clone());
            bucket
        };

        let mut bucket = bucket_arc.lock().await;
        let now = Instant::now();
        if now.duration_since(bucket.start_time) > Duration::from_secs(60) {
            bucket.count = 1;
            bucket.start_time = now;
            Ok(())
        } else if bucket.count < limit {
            bucket.count += 1;
            Ok(())
        } else {
            Err(Duration::from_secs(60).saturating_sub(now.duration_since(bucket.start_time)))
        }
    }

    pub async fn acquire_sse_permit(&self, ip: IpAddr) -> Option<SsePermit> {
        if self.config.sse_max_concurrent == 0 {
            return None;
        }

        let semaphore = if let Some(semaphore) = self.limits.sse_concurrency.get(&ip) {
            semaphore
        } else {
            let semaphore = Arc::new(Semaphore::new(self.config.sse_max_concurrent as usize));
            self.limits.sse_concurrency.insert(ip, semaphore.clone());
            semaphore
        };

        let permit: OwnedSemaphorePermit = semaphore.acquire_owned().await.ok()?;
        Some(SsePermit(Arc::new(Mutex::new(Some(permit)))))
    }
}

pub async fn transactions_limit(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(context): axum::extract::State<crate::rest_api::context::HandlerContext>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let manager = context.rate_limit_manager();
    match manager
        .check_limit(addr.ip(), &manager.limits.transactions, manager.config.transactions_per_min)
        .await
    {
        Ok(_) => next.run(request).await,
        Err(wait) => too_many_requests(wait),
    }
}

pub async fn substates_fetch_limit(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(context): axum::extract::State<crate::rest_api::context::HandlerContext>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let manager = context.rate_limit_manager();
    match manager
        .check_limit(
            addr.ip(),
            &manager.limits.substates_fetch,
            manager.config.substates_fetch_per_min,
        )
        .await
    {
        Ok(_) => next.run(request).await,
        Err(wait) => too_many_requests(wait),
    }
}

pub async fn utxos_fetch_limit(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(context): axum::extract::State<crate::rest_api::context::HandlerContext>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let manager = context.rate_limit_manager();
    match manager
        .check_limit(addr.ip(), &manager.limits.utxos_fetch, manager.config.utxos_fetch_per_min)
        .await
    {
        Ok(_) => next.run(request).await,
        Err(wait) => too_many_requests(wait),
    }
}

pub async fn non_fungibles_limit(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(context): axum::extract::State<crate::rest_api::context::HandlerContext>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let manager = context.rate_limit_manager();
    match manager
        .check_limit(
            addr.ip(),
            &manager.limits.non_fungibles,
            manager.config.non_fungibles_per_min,
        )
        .await
    {
        Ok(_) => next.run(request).await,
        Err(wait) => too_many_requests(wait),
    }
}

pub async fn dry_run_limit(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(context): axum::extract::State<crate::rest_api::context::HandlerContext>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let manager = context.rate_limit_manager();
    match manager
        .check_limit(addr.ip(), &manager.limits.dry_run, manager.config.dry_run_per_min)
        .await
    {
        Ok(_) => next.run(request).await,
        Err(wait) => too_many_requests(wait),
    }
}

pub async fn sse_limit(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(context): axum::extract::State<crate::rest_api::context::HandlerContext>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let manager = context.rate_limit_manager();
    if let Some(permit) = manager.acquire_sse_permit(addr.ip()).await {
        let mut response = next.run(request).await;
        response.extensions_mut().insert(permit);
        response
    } else {
        too_many_requests(Duration::from_secs(60))
    }
}

fn too_many_requests(wait: Duration) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("Retry-After", wait.as_secs().to_string())],
        Json(json!({
            "error": format!("Too many requests. Please try again in {} seconds", wait.as_secs()),
        })),
    )
        .into_response()
}
