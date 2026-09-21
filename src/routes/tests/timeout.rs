use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::{get, post, put},
};
use tower::ServiceExt;

use super::request_deadline;

async fn slow() -> StatusCode {
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    StatusCode::OK
}

#[tokio::test]
async fn test_archive_deadline_does_not_inherit_ordinary_timeout() {
    let app = Router::new()
        .route("/api/archive/object/parts/1", put(slow))
        .route("/api/archive/object/complete", post(slow))
        .route("/api/me", get(slow))
        .layer(middleware::from_fn_with_state((0, 1), request_deadline));
    for (method, path, expected) in [
        ("PUT", "/api/archive/object/parts/1", StatusCode::OK),
        ("POST", "/api/archive/object/complete", StatusCode::OK),
        ("GET", "/api/me", StatusCode::REQUEST_TIMEOUT),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
}
