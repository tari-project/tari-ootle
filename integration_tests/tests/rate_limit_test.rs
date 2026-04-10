# Unit Tests for Rate Limiting on POST /transactions

use actix_web::{test, web, App};
use actix_web::http::{StatusCode};
use your_rate_limiting_module::setup_rate_limiting;

#[actix_web::test]
async fn test_post_transactions_rate_limit() {
    let app = test::init_service(App::new().configure(setup_rate_limiting)).await;

    // Test 20 requests within 1 minute
    for _ in 0..20 {
        let req = test::TestRequest::post().uri("/transactions").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    // Test 21st request should fail
    let req = test::TestRequest::post().uri("/transactions").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}