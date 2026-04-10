// Import the rate limiting middleware structure
use crate::rest_api::middleware::rate_limiting::RateLimit;

// Integrating the middleware into the routes
// Example for POST /transactions
.route("/transactions", RateLimit {}.layer(post(handlers::transactions::submit_transaction)))
// Rate limiting for other endpoints as specified
.route("/substates/fetch", RateLimit {}.layer(post(handlers::substates::fetch_substates)))
.route("/utxos/fetch", RateLimit {}.layer(post(handlers::utxos::fetch_utxos)))
.route("/non-fungibles", RateLimit {}.layer(get(handlers::nfts::get_non_fungibles)))
// Adding any SSE connections if necessary
