# Integration Tests for Rate Limiting

use actix_web::{test, web, App};
use actix_web::http::{StatusCode};
use your_rate_limiting_module::setup_rate_limiting;

#[actix_web::test]
async fn test_integration_rate_limits() {
    let app = test::init_service(App::new().configure(setup_rate_limiting)).await;

    // Test POST /substates/fetch rate limit (30 req/min)
    for _ in 0..30 {
        let req = test::TestRequest::post().uri("/substates/fetch").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    // 31st request should fail
    let req = test::TestRequest::post().uri("/substates/fetch").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // Test POST /utxos/fetch rate limit (15 req/min)
    for _ in 0..15 {
        let req = test::TestRequest::post().uri("/utxos/fetch").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    // 16th request should fail
    let req = test::TestRequest::post().uri("/utxos/fetch").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // Test GET /non-fungibles rate limit (30 req/min)
    for _ in 0..30 {
        let req = test::TestRequest::get().uri("/non-fungibles").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    // 31st request should fail
    let req = test::TestRequest::get().uri("/non-fungibles").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // Allow time for the rate limits to reset
    // Normally, we may include tests for SSE stream endpoints in accordance with concurrency rules.
}