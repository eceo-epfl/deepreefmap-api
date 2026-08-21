use axum::Router;
use axum::body::Body;
use axum::http::HeaderMap;
use http_body_util::BodyExt;
use tower::ServiceExt;

use deepreefmap_api::common::contract::{CONTRACT_HEADER, SECTIONS_HEADER};

/// What a current client declares, and what every helper here sends unless a test hands
/// over its own header set.
#[must_use]
pub fn negotiation() -> Vec<(String, String)> {
    vec![
        (CONTRACT_HEADER.to_string(), "1-1".to_string()),
        (
            SECTIONS_HEADER.to_string(),
            deepreefmap_api::routes::private::sync::schema::sections().join(","),
        ),
    ]
}

async fn send(app: &Router, req: axum::http::Request<Body>) -> (u16, HeaderMap, String) {
    let response = app.clone().oneshot(req).await.expect("router responds");
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body reads")
        .to_bytes();
    (status, headers, String::from_utf8_lossy(&bytes).to_string())
}

fn build(
    method: &str,
    uri: &str,
    token: Option<&str>,
    headers: &[(String, String)],
) -> axum::http::request::Builder {
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    builder
}

/// A GET declaring exactly `headers`, so a test can omit the negotiation or misspell it.
///
/// # Panics
///
/// Panics when the request cannot be built.
pub async fn get_declaring(
    app: &Router,
    uri: &str,
    token: Option<&str>,
    headers: &[(String, String)],
) -> (u16, HeaderMap, String) {
    let req = build("GET", uri, token, headers)
        .body(Body::empty())
        .expect("request builds");
    send(app, req).await
}

/// A POST declaring exactly `headers`.
///
/// # Panics
///
/// Panics when the request cannot be built.
pub async fn post_declaring(
    app: &Router,
    uri: &str,
    body: &serde_json::Value,
    token: Option<&str>,
    headers: &[(String, String)],
) -> (u16, HeaderMap, String) {
    let req = build("POST", uri, token, headers)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds");
    send(app, req).await
}

/// # Panics
///
/// Panics when the request cannot be built.
pub async fn get(app: &Router, uri: &str, token: Option<&str>) -> (u16, String) {
    let (status, _, body) = get_declaring(app, uri, token, &negotiation()).await;
    (status, body)
}

/// # Panics
///
/// Panics when the response is not JSON, printing the body so the failure is legible.
pub async fn get_json(app: &Router, uri: &str, token: Option<&str>) -> (u16, serde_json::Value) {
    let (status, body) = get(app, uri, token).await;
    let json = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("GET {uri} returned non-JSON: {e}\nBody: {body}"));
    (status, json)
}

/// A GET returning the response headers too, for assertions on `Content-Range`.
///
/// # Panics
///
/// Panics when the response is not JSON.
pub async fn get_json_with_headers(
    app: &Router,
    uri: &str,
    token: Option<&str>,
) -> (u16, HeaderMap, serde_json::Value) {
    let (status, headers, body) = get_declaring(app, uri, token, &negotiation()).await;
    let json = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("GET {uri} returned non-JSON: {e}\nBody: {body}"));
    (status, headers, json)
}

/// # Panics
///
/// Panics when the request cannot be built.
pub async fn post(
    app: &Router,
    uri: &str,
    body: &serde_json::Value,
    token: Option<&str>,
) -> (u16, String) {
    let (status, _, text) = post_declaring(app, uri, body, token, &negotiation()).await;
    (status, text)
}

/// # Panics
///
/// Panics when the response is not JSON.
pub async fn post_json(
    app: &Router,
    uri: &str,
    body: &serde_json::Value,
    token: Option<&str>,
) -> (u16, serde_json::Value) {
    let (status, text) = post(app, uri, body, token).await;
    let json = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("POST {uri} returned non-JSON: {e}\nBody: {text}"));
    (status, json)
}

/// # Panics
///
/// Panics when the request cannot be built.
pub async fn put(
    app: &Router,
    uri: &str,
    body: &serde_json::Value,
    token: Option<&str>,
) -> (u16, String) {
    let req = build("PUT", uri, token, &negotiation())
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds");
    let (status, _, text) = send(app, req).await;
    (status, text)
}

/// # Panics
///
/// Panics when the request cannot be built.
pub async fn delete(app: &Router, uri: &str, token: Option<&str>) -> (u16, String) {
    delete_with_body(app, uri, None, token).await
}

/// A batch delete carries its ids in the body.
///
/// # Panics
///
/// Panics when the request cannot be built.
pub async fn delete_with_body(
    app: &Router,
    uri: &str,
    body: Option<&serde_json::Value>,
    token: Option<&str>,
) -> (u16, String) {
    let payload = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    let req = build("DELETE", uri, token, &negotiation())
        .header("Content-Type", "application/json")
        .body(payload)
        .expect("request builds");
    let (status, _, text) = send(app, req).await;
    (status, text)
}

/// Enrol a device and return its bearer token. The device takes the name seeded on
/// its connect code.
///
/// # Panics
///
/// Panics when enrolment does not succeed, so a broken harness fails loudly rather
/// than every later assertion failing on a missing token.
pub async fn enrol_device(app: &Router, code: &str) -> String {
    let (status, body) = post_json(
        app,
        "/api/enrol",
        &serde_json::json!({ "code": code }),
        None,
    )
    .await;
    assert_eq!(status, 200, "enrolment failed: {body}");
    body["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}
