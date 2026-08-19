//! Contract negotiation across the wire: what a client declares, what it is served, and
//! what a server that shares no version with it says.

#[allow(dead_code)]
mod common;

use common::*;

use deepreefmap_api::common::contract::{CONTRACT_HEADER, SECTIONS_HEADER};
use deepreefmap_api::routes::sync::schema::{CONTRACT_VERSION, MIN_CONTRACT_VERSION};

fn declaring(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect()
}

fn site_row(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "description": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn transect_row(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "description": "",
        "start_lat": 12.0, "start_lon": 43.0,
        "end_lat": 12.001, "end_lon": 43.001,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn push_body(sections: &serde_json::Value, contract_version: u32) -> serde_json::Value {
    serde_json::json!({ "contract_version": contract_version, "sections": sections })
}

async fn one_device(app: &axum::Router, db: &sea_orm::DatabaseConnection) -> String {
    let code = seed_connect_code(db, "alice").await;
    enrol_device(app, &code, "Alice laptop").await
}

fn server_range(headers: &axum::http::HeaderMap, on: &str) -> String {
    headers
        .get(CONTRACT_HEADER)
        .unwrap_or_else(|| panic!("no server range stamped on {on}"))
        .to_str()
        .expect("the range is text")
        .to_string()
}

#[tokio::test]
async fn test_a_disjoint_range_is_refused_and_writes_nothing() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let token = one_device(&app, &db).await;

    let ahead = format!("{}-{}", CONTRACT_VERSION + 2, CONTRACT_VERSION + 3);
    let (status, headers, body) = post_declaring(
        &app,
        "/api/sync/push",
        &push_body(
            &serde_json::json!({
                "sites": [site_row("33333333-3333-4333-8333-333333333333", "Refused")]
            }),
            1,
        ),
        Some(&token),
        &declaring(&[(CONTRACT_HEADER, &ahead)]),
    )
    .await;

    assert_eq!(status, 400, "{body}");
    let refusal: serde_json::Value = serde_json::from_str(&body).expect("a JSON error body");
    let message = refusal["error"].as_str().expect("the usual error shape");
    assert!(
        message.contains(&format!("{MIN_CONTRACT_VERSION}-{CONTRACT_VERSION}"))
            && message.contains(&ahead),
        "the refusal names both ranges: {message}"
    );
    assert_eq!(
        server_range(&headers, "the gate's refusal"),
        format!("{MIN_CONTRACT_VERSION}-{CONTRACT_VERSION}"),
        "the gate's own refusal is stamped"
    );

    let sites: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM site").await;
    assert_eq!(sites, 0, "a refused push wrote rows");
}

#[tokio::test]
async fn test_a_narrowed_client_is_served_the_intersection() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let token = one_device(&app, &db).await;

    seed_site(&db, "44444444-4444-4444-8444-444444444444", "Narrowed").await;

    // A section this build has never heard of is not a refusal: it is a newer client.
    let (status, _, body) = get_declaring(
        &app,
        "/api/sync/pull",
        Some(&token),
        &declaring(&[
            (CONTRACT_HEADER, "1-1"),
            (SECTIONS_HEADER, "sites,soundscapes"),
        ]),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let pulled: serde_json::Value = serde_json::from_str(&body).expect("JSON");

    assert_eq!(pulled["sections"]["sites"].as_array().unwrap().len(), 1);
    let omitted: Vec<&str> = pulled["omitted_sections"]
        .as_array()
        .expect("omitted_sections listed")
        .iter()
        .map(|v| v.as_str().expect("a section name"))
        .collect();
    // A device pulls sites, campaigns, transects and presets; the client kept only
    // the first.
    assert_eq!(omitted, ["campaigns", "transects", "presets"], "{pulled}");
}

#[tokio::test]
async fn test_an_undeclared_client_pulls_what_it_always_did() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let token = one_device(&app, &db).await;

    seed_site(&db, "55555555-5555-4555-8555-555555555555", "Legacy").await;

    let (status, _, body) = get_declaring(&app, "/api/sync/pull", Some(&token), &[]).await;
    assert_eq!(status, 200, "{body}");
    let pulled: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(pulled["contract_version"], 1);
    assert_eq!(pulled["sections"]["sites"].as_array().unwrap().len(), 1);
    assert!(
        pulled["omitted_sections"].as_array().unwrap().is_empty(),
        "an absent header narrows nothing: {pulled}"
    );

    // A malformed value is absent too, or a truncating proxy bricks a working laptop.
    let (status, _, body) = get_declaring(
        &app,
        "/api/sync/pull",
        Some(&token),
        &declaring(&[(CONTRACT_HEADER, "1-"), (SECTIONS_HEADER, "SITES ONLY")]),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let pulled: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    assert!(pulled["omitted_sections"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_a_push_at_the_negotiated_down_version_is_accepted() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let token = one_device(&app, &db).await;

    // A client ahead of this server agrees on the server's maximum and writes that.
    let newer = format!("{MIN_CONTRACT_VERSION}-{}", CONTRACT_VERSION + 4);
    let (status, _, body) = post_declaring(
        &app,
        "/api/sync/push",
        &push_body(
            &serde_json::json!({
                "transects": [transect_row("66666666-6666-4666-8666-666666666666", "Agreed")]
            }),
            CONTRACT_VERSION,
        ),
        Some(&token),
        &declaring(&[(CONTRACT_HEADER, &newer)]),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let pushed: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(pushed["contract_version"], CONTRACT_VERSION);
    assert_eq!(pushed["sections"]["transects"]["applied"], 1);

    // The server's own maximum, declared by a client that cannot read it, is still refused.
    let (status, _, body) = post_declaring(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({}), CONTRACT_VERSION + 4),
        Some(&token),
        &declaring(&[(CONTRACT_HEADER, &newer)]),
    )
    .await;
    assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn test_every_sync_response_carries_the_agreed_version() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());

    let code = seed_connect_code(&db, "alice").await;
    let (status, enrolled) = post_json(
        &app,
        "/api/enrol",
        &serde_json::json!({ "code": code, "device_name": "Alice laptop" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "enrolment failed: {enrolled}");
    assert_eq!(enrolled["contract_version"], CONTRACT_VERSION);
    let token = enrolled["token"].as_str().expect("a token").to_string();

    let (status, pushed) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({}), 1),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{pushed}");
    assert_eq!(pushed["contract_version"], CONTRACT_VERSION);

    let (status, pulled) = get_json(&app, "/api/sync/pull", Some(&token)).await;
    assert_eq!(status, 200, "{pulled}");
    assert_eq!(pulled["contract_version"], CONTRACT_VERSION);
}

#[tokio::test]
async fn test_the_server_range_is_stamped_on_errors_too() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let expected = format!("{MIN_CONTRACT_VERSION}-{CONTRACT_VERSION}");

    let (status, headers, _) = get_declaring(&app, "/api/sync/pull", None, &negotiation()).await;
    assert_eq!(status, 401);
    assert_eq!(server_range(&headers, "a 401"), expected);

    let (status, headers, _) =
        get_declaring(&app, "/api/no-such-route", None, &negotiation()).await;
    assert_eq!(status, 404);
    assert_eq!(server_range(&headers, "a 404"), expected);
}
