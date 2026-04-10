use actix_web::{dev::{Service, ServiceRequest, ServiceResponse, Transform}, Error};
use std::{collections::HashMap, sync::{Arc, Mutex}, time::{Duration, Instant}};

// Structure to represent each client's rate limiting state
struct RateLimiter {
    requests: HashMap<String, (u32, Instant)>, // (request count, timestamp)
}

impl RateLimiter {
    fn new() -> Self {
        RateLimiter {
            requests: HashMap::new(),
        }
    }

    fn is_rate_limited(&mut self, ip: &String, max_requests: u32, duration: Duration) -> bool {
        let current_time = Instant::now();
        let (count, timestamp) = self.requests.entry(ip.clone()).or_insert((0, current_time));

        // Check if duration has passed
        if current_time.duration_since(*timestamp) > duration {
            // Reset count and timestamp
            *count = 0;
            *timestamp = current_time;
        }

        // Increment count
        *count += 1;
        *count > max_requests  // Return true if limit exceeded
    }
}

pub struct RateLimit;

impl<S> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error>,
{
    type Response = ServiceResponse;
    type Error = Error;
    type InitError = ();

    type Transform = RateLimitMiddleware<S>;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Transform, Self::InitError>>>>;

    fn new_transform(&self, service: S) -> Self::Future {
        Box::pin(async { Ok(RateLimitMiddleware { service }) })
    }
}

pub struct RateLimitMiddleware<S> {
    service: S,
}

impl<S> Service<ServiceRequest> for RateLimitMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error>,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>>>>;

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let ip = req.peer_addr().map(|addr| addr.ip().to_string()).unwrap_or_else(|| "unknown".into());
        let max_requests = 20; // Example limit, this could be configurable
        let duration = Duration::new(60, 0);

        // Here we need a way to persist state. For simplicity, we assume it's stored somewhere shared.
        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new()));
        let mut rate_limiter_clone = rate_limiter.clone();

        Box::pin(async move {
            if rate_limiter_clone.lock().unwrap().is_rate_limited(&ip, max_requests, duration) {
                return Ok(req.error_response(StatusCode::TOO_MANY_REQUESTS));
            }
            self.service.call(req).await
        })
    }
}