# Add Rate Limiting to Indexer API
> Last updated: 2026-04-10
## Overview
This feature adds rate limiting to the indexer REST API to protect against excessive load and potential abuse. It implements configurable limits on various endpoints, ensuring that the system remains stable under high usage.
## How It Works
Rate limiting is implemented in `applications/tari_indexer/src/rest_api/middleware/rate_limiting.rs` using a token-bucket algorithm. The limits are configurable via `applications/tari_indexer/src/config.rs`, allowing for adjustments based on operational needs. The middleware is integrated into the API request handling in `applications/tari_indexer/src/rest_api/middleware.rs` and `applications/tari_indexer/src/rest_api/server.rs`.
## Configuration
Rate limit values can be set in the indexer configuration file, including limits for each endpoint and concurrent connection limits for SSE streams.
## Usage
To test the rate limiting, send requests to the specified endpoints and observe the behavior when limits are exceeded.
## References
- Closes issue #1997