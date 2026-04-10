use axum::Router;
use crate::rest_api::middleware::RateLimiter;

// Add this middleware to your REST API setup in `lib.rs`

let app = Router::new()
    .route("/transactions", post(submit_transaction))
    .route("/transactions/dry-run", post(submit_transaction_dry_run))
    .layer(middleware::from_fn(RateLimiter::handle_rate_limiting)); // Apply the rate limiting middleware here